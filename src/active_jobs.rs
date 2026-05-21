use std::process::Command;

use crate::command_ext::{run_with_timeout, SLURM_COMMAND_TIMEOUT};
use crate::gres_parser::GresParser;
use crate::models::{ActiveJobInfo, JobState};

pub struct ActiveJobsCollector;

impl ActiveJobsCollector {
    /// Query squeue for all currently RUNNING and PENDING jobs.
    ///
    /// Format string: %i=JobID %u=User %P=Partition %T=State %M=Elapsed
    ///                %l=TimeLimit %C=NumCPUs %b=TRESPerNode %N=NodeList %r=Reason
    pub fn get_active_jobs(
        partition: Option<&str>,
        user: Option<&str>,
        debug: bool,
    ) -> Vec<ActiveJobInfo> {
        let mut jobs = Vec::new();

        let mut cmd = Command::new("squeue");
        cmd.args([
            "--noheader",
            "-t",
            "RUNNING,PENDING",
            "-o",
            "%i|%u|%P|%T|%M|%l|%C|%b|%N|%r",
        ]);

        if let Some(p) = partition {
            cmd.args(["-p", p]);
        }
        if let Some(u) = user {
            cmd.args(["-u", u]);
        }

        if debug {
            eprintln!("Debug: Running squeue for active (RUNNING+PENDING) jobs");
        }

        match run_with_timeout(cmd, SLURM_COMMAND_TIMEOUT) {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    let fields: Vec<&str> = line.splitn(10, '|').collect();
                    if fields.len() < 10 {
                        if debug {
                            eprintln!("Warning: unexpected squeue line: {}", line);
                        }
                        continue;
                    }
                    if let Some(job) = Self::parse_line(&fields) {
                        if debug && jobs.len() < 3 {
                            eprintln!(
                                "Debug: Active job {} ({}) user={} gpus={}",
                                job.job_id,
                                job.state.as_str(),
                                job.user,
                                job.gpu_request
                            );
                        }
                        jobs.push(job);
                    }
                }
            }
            Ok(output) => {
                if debug {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    eprintln!("Warning: squeue failed: {}", stderr);
                }
            }
            Err(e) => {
                if debug {
                    eprintln!("Warning: Failed to run squeue: {}", e);
                }
            }
        }

        jobs
    }

    fn parse_line(fields: &[&str]) -> Option<ActiveJobInfo> {
        let raw_job_id = fields[0];
        let job_id = raw_job_id
            .split('_')
            .next()
            .unwrap_or(raw_job_id)
            .to_string();

        let user = fields[1].to_string();
        let partition = fields[2].to_string();
        let state = JobState::from(fields[3]);

        let elapsed = if state.is_pending() {
            "---".to_string()
        } else {
            fields[4].to_string()
        };

        let time_limit = clean_field(fields[5]).unwrap_or("---").to_string();

        let cpu_request: i32 = clean_field(fields[6])
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let tres_per_node = clean_field(fields[7]).unwrap_or("");

        let node = clean_field(fields[8]).map(|s| s.to_string());

        let reason = clean_field(fields[9])
            .filter(|&r| r != "None")
            .unwrap_or("")
            .to_string();

        let mut gpu_request = 0i32;
        let mut gpu_type: Option<String> = None;
        if !tres_per_node.is_empty() {
            if let Some((gtype, count)) =
                GresParser::parse_gres_string(tres_per_node).into_iter().next()
            {
                gpu_request = count;
                if gtype != "gpu" {
                    gpu_type = Some(gtype);
                }
            }
        }

        Some(ActiveJobInfo {
            job_id,
            user,
            partition,
            state,
            elapsed,
            time_limit,
            cpu_request,
            gpu_request,
            gpu_type,
            node,
            reason,
        })
    }
}

fn clean_field(s: &str) -> Option<&str> {
    match s {
        "" | "N/A" | "(null)" | "None" => None,
        other => Some(other),
    }
}
