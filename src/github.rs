use serde::Deserialize;
use std::collections::{HashMap, VecDeque};

use crate::errors::Result;
use crate::pipeline::{compute_task_hash, Pipeline, Task};

// ── GitHub Actions YAML structures ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct Workflow {
    #[serde(default)]
    jobs: HashMap<String, Job>,
}

#[derive(Debug, Deserialize)]
struct Job {
    #[serde(default)]
    steps: Vec<Step>,
    /// `needs` can be a single string or a list of strings.
    #[serde(default, deserialize_with = "string_or_vec")]
    needs: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Step {
    pub run: Option<String>,
    pub name: Option<String>,
}

// ── Parser ────────────────────────────────────────────────────────────────────

/// Convert a GitHub Actions workflow YAML into a native `Pipeline`.
///
/// Mapping rules:
/// * Each `run:` step becomes one `Task`.
/// * Steps within a job are sequential (each depends on the previous).
/// * `needs:` on a job causes the job's first step to depend on the **last
///   step** of each required job. Jobs are processed in topological order so
///   cross-job tail IDs are always known before they are referenced.
pub fn parse_github_workflow(content: &str) -> Result<Pipeline> {
    let workflow: Workflow = serde_yaml::from_str(content)?;

    // Process jobs in dependency order so `job_tail` is always populated
    // before a dependent job tries to look up a required job's tail task.
    let job_order = topological_job_order(&workflow.jobs);

    let mut tasks: Vec<Task> = Vec::new();
    let mut job_tail: HashMap<String, String> = HashMap::new();

    for job_name in &job_order {
        let job = &workflow.jobs[job_name];
        let mut prev: Option<String> = None;

        for (idx, step) in job.steps.iter().enumerate() {
            let run_cmd = match &step.run {
                Some(r) => r.trim().to_string(),
                None => continue, // `uses:` steps are skipped
            };

            let label = step
                .name
                .as_deref()
                .unwrap_or("")
                .trim()
                .replace([' ', '-', '/'], "_");

            let task_id = if label.is_empty() {
                format!("{}_step_{}", job_name, idx)
            } else {
                format!("{}__{}", job_name, label)
            };

            let mut depends_on: Vec<String> = Vec::new();

            // Sequential within-job dependency
            if let Some(p) = prev.clone() {
                depends_on.push(p);
            }

            // Cross-job dependency via `needs:` — first step of this job only.
            // Because we process in topological order, job_tail always has the
            // required entry by the time we reach this job.
            if prev.is_none() {
                for needed in &job.needs {
                    if let Some(tail) = job_tail.get(needed) {
                        depends_on.push(tail.clone());
                    }
                }
            }

            prev = Some(task_id.clone());
            tasks.push(Task {
                id: task_id,
                command: run_cmd,
                depends_on,
                hash: None,
            });
        }

        if let Some(tail_id) = prev {
            job_tail.insert(job_name.clone(), tail_id);
        }
    }

    for task in &mut tasks {
        task.hash = Some(compute_task_hash(&task.command, &task.depends_on));
    }

    Ok(Pipeline { tasks })
}

/// Return job names sorted in topological order (required jobs come first).
/// Jobs with no `needs` are sorted alphabetically for determinism.
fn topological_job_order(jobs: &HashMap<String, Job>) -> Vec<String> {
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new(); // needed → jobs that need it

    for (name, job) in jobs {
        in_degree.entry(name.as_str()).or_insert(0);
        for needed in &job.needs {
            dependents
                .entry(needed.as_str())
                .or_default()
                .push(name.as_str());
            *in_degree.entry(name.as_str()).or_insert(0) += 1;
        }
    }

    // Start with all root jobs (sorted for determinism)
    let mut queue: VecDeque<&str> = {
        let mut roots: Vec<&str> = in_degree
            .iter()
            .filter(|(_, &d)| d == 0)
            .map(|(&n, _)| n)
            .collect();
        roots.sort_unstable();
        roots.into_iter().collect()
    };

    let mut order: Vec<String> = Vec::new();
    while let Some(node) = queue.pop_front() {
        order.push(node.to_string());
        if let Some(deps) = dependents.get(node) {
            let mut next: Vec<&str> = deps.clone();
            next.sort_unstable();
            for dep in next {
                let d = in_degree.get_mut(dep).unwrap();
                *d -= 1;
                if *d == 0 {
                    queue.push_back(dep);
                }
            }
        }
    }

    // Append any jobs not reachable (e.g. cycles or isolated jobs)
    for name in jobs.keys() {
        if !order.contains(name) {
            order.push(name.clone());
        }
    }

    order
}

// ── Serde helper: accept "string" or ["string", …] ───────────────────────────

fn string_or_vec<'de, D>(de: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{SeqAccess, Visitor};
    use std::fmt;

    struct Sv;

    impl<'de> Visitor<'de> for Sv {
        type Value = Vec<String>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a string or a sequence of strings")
        }

        fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Vec<String>, E> {
            Ok(vec![v.to_owned()])
        }

        fn visit_string<E: serde::de::Error>(
            self,
            v: String,
        ) -> std::result::Result<Vec<String>, E> {
            Ok(vec![v])
        }

        fn visit_seq<A: SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Vec<String>, A::Error> {
            let mut out = Vec::new();
            while let Some(s) = seq.next_element::<String>()? {
                out.push(s);
            }
            Ok(out)
        }
    }

    de.deserialize_any(Sv)
}
