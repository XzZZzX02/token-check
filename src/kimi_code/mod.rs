use crate::date::CivilDate;
use crate::model::{ReportData, SessionMeta, Source, ToolEvent, Usage, UsageEvent};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const WIRE_FILE_NAME: &str = "wire.jsonl";
const STATE_FILE_NAME: &str = "state.json";

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum WireRecord {
    #[serde(rename = "context.append_loop_event")]
    LoopEvent {
        event: LoopEvent,
        #[serde(default)]
        time: Option<i64>,
    },
    #[serde(rename = "llm.request")]
    LlmRequest {
        #[serde(default)]
        model: Option<String>,
        #[serde(default, rename = "modelAlias")]
        model_alias: Option<String>,
        #[serde(default, rename = "turnStep")]
        turn_step: Option<String>,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum LoopEvent {
    #[serde(rename = "step.end")]
    StepEnd {
        uuid: String,
        #[serde(rename = "turnId")]
        turn_id: String,
        step: u64,
        #[serde(default)]
        usage: Option<KimiUsage>,
    },
    #[serde(rename = "tool.call")]
    ToolCall { uuid: String, name: String },
    #[serde(other)]
    Other,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KimiUsage {
    #[serde(default)]
    input_other: u64,
    #[serde(default)]
    output: u64,
    #[serde(default)]
    input_cache_read: u64,
    #[serde(default)]
    input_cache_creation: u64,
}

impl KimiUsage {
    fn into_usage(self) -> Usage {
        Usage {
            input: self.input_other,
            cached_input: self.input_cache_read,
            cache_creation_input: self.input_cache_creation,
            cache_creation_input_5m: 0,
            cache_creation_input_1h: 0,
            output: self.output,
            reasoning_output: 0,
            total: 0,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionState {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    created_at: Option<i64>,
}

struct WireFileData {
    usage_events: Vec<UsageEvent>,
    tool_events: Vec<ToolEvent>,
    warnings: Vec<String>,
    last_model: Option<String>,
}

pub fn collect(sessions_root: &Path) -> Result<ReportData> {
    let mut data = ReportData::default();
    if !sessions_root.exists() {
        data.warnings.push(format!(
            "Kimi Code data directory not found: {}",
            sessions_root.display()
        ));
        return Ok(data);
    }

    let mut sessions: BTreeMap<PathBuf, SessionMeta> = BTreeMap::new();
    for entry in WalkDir::new(sessions_root) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                data.warnings
                    .push(format!("Failed to walk Kimi Code data: {err}"));
                continue;
            }
        };
        if !entry.file_type().is_file() || entry.file_name() != WIRE_FILE_NAME {
            continue;
        }
        let Some(session_dir) = session_dir_for(entry.path()) else {
            continue;
        };
        let session_id = dir_name(session_dir);
        let meta = sessions
            .entry(session_dir.to_path_buf())
            .or_insert_with(|| session_meta(session_dir, &session_id));

        match collect_file(entry.path(), &meta.session_id, &meta.project) {
            Ok(file_data) => {
                if let Some(model) = file_data.last_model {
                    meta.model = model;
                }
                data.usage_events.extend(file_data.usage_events);
                data.tool_events.extend(file_data.tool_events);
                data.warnings.extend(file_data.warnings);
            }
            Err(err) => data.warnings.push(format!(
                "Failed to read Kimi Code file {}: {err:#}",
                entry.path().display()
            )),
        }
    }

    data.sessions.extend(sessions.into_values());
    Ok(data)
}

fn session_dir_for(wire_path: &Path) -> Option<&Path> {
    // wire.jsonl -> <agent> -> agents -> <session dir>
    wire_path.parent()?.parent()?.parent()
}

fn dir_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_string()
}

fn session_meta(session_dir: &Path, session_id: &str) -> SessionMeta {
    let state_path = session_dir.join(STATE_FILE_NAME);
    let state = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|content| serde_json::from_str::<SessionState>(&content).ok());

    let (id, project, date) = match state {
        Some(state) => (
            state.id.unwrap_or_else(|| session_id.to_string()),
            state.cwd.unwrap_or_else(|| String::from("unknown")),
            state
                .created_at
                .map(date_from_epoch_millis)
                .unwrap_or_else(|| String::from("unknown")),
        ),
        None => (
            session_id.to_string(),
            String::from("unknown"),
            String::from("unknown"),
        ),
    };

    SessionMeta {
        source: Source::KimiCode,
        session_id: id,
        date,
        project,
        model: String::from("unknown"),
    }
}

fn collect_file(path: &Path, session_id: &str, project: &str) -> Result<WireFileData> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut models_by_turn_step: BTreeMap<String, String> = BTreeMap::new();
    let mut usage_events = Vec::new();
    let mut tool_events = Vec::new();
    let mut warnings = Vec::new();
    let mut last_model: Option<String> = None;

    for (line_index, line) in reader.lines().enumerate() {
        let line =
            line.with_context(|| format!("read line {} in {}", line_index + 1, path.display()))?;
        if line.trim().is_empty() {
            continue;
        }

        let record = match serde_json::from_str::<WireRecord>(&line) {
            Ok(record) => record,
            Err(err) => {
                warnings.push(format!(
                    "Invalid JSON in {} line {}: {err}",
                    path.display(),
                    line_index + 1
                ));
                continue;
            }
        };

        match record {
            WireRecord::LlmRequest {
                model,
                model_alias,
                turn_step,
            } => {
                let model = resolve_model(model, model_alias);
                if let Some(turn_step) = turn_step {
                    models_by_turn_step.insert(turn_step, model.clone());
                }
                last_model = Some(model);
            }
            WireRecord::LoopEvent { event, time } => match event {
                LoopEvent::StepEnd {
                    uuid,
                    turn_id,
                    step,
                    usage,
                } => {
                    let Some(usage) = usage else {
                        continue;
                    };
                    let usage = usage.into_usage();
                    if usage.computed_total() == 0 {
                        continue;
                    }
                    let model = models_by_turn_step
                        .get(&format!("{turn_id}.{step}"))
                        .cloned()
                        .unwrap_or_else(|| String::from("unknown"));
                    usage_events.push(UsageEvent {
                        source: Source::KimiCode,
                        event_id: uuid,
                        session_id: session_id.to_string(),
                        date: time
                            .map(date_from_epoch_millis)
                            .unwrap_or_else(|| String::from("unknown")),
                        project: project.to_string(),
                        model,
                        usage,
                    });
                }
                LoopEvent::ToolCall { uuid, name } => {
                    tool_events.push(ToolEvent {
                        source: Source::KimiCode,
                        event_id: uuid,
                        date: time
                            .map(date_from_epoch_millis)
                            .unwrap_or_else(|| String::from("unknown")),
                        project: project.to_string(),
                        tool: name,
                    });
                }
                LoopEvent::Other => {}
            },
            WireRecord::Other => {}
        }
    }

    Ok(WireFileData {
        usage_events,
        tool_events,
        warnings,
        last_model,
    })
}

fn resolve_model(model: Option<String>, model_alias: Option<String>) -> String {
    if let Some(model) = model.filter(|model| !model.trim().is_empty()) {
        return model;
    }
    model_alias
        .and_then(|alias| alias.rsplit('/').next().map(str::to_string))
        .filter(|model| !model.trim().is_empty())
        .unwrap_or_else(|| String::from("unknown"))
}

fn date_from_epoch_millis(time: i64) -> String {
    let epoch = CivilDate {
        year: 1970,
        month: 1,
        day: 1,
    };
    epoch.add_days(time.div_euclid(86_400_000)).to_string()
}

#[cfg(test)]
mod tests {
    use super::collect;
    use crate::model::Source;
    use serde_json::json;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn collects_kimi_usage_tool_events_and_session_meta() {
        // Given
        let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("tokencheck-kimi-{}-{counter}", std::process::id()));
        let session_dir = root.join("wd_project_deadbeef/session_abc-123");
        let main_agents = session_dir.join("agents/main");
        let sub_agents = session_dir.join("agents/agent-0");
        fs::create_dir_all(&main_agents).unwrap();
        fs::create_dir_all(&sub_agents).unwrap();
        fs::write(
            session_dir.join("state.json"),
            json!({
                "id": "session_abc-123",
                "version": 2,
                "cwd": "/tmp/kimi-project",
                "createdAt": 1785312000000i64,
                "updatedAt": 1785312180000i64
            })
            .to_string(),
        )
        .unwrap();
        let main_wire = [
            json!({
                "type": "llm.request",
                "agentId": "main",
                "model": "k3",
                "modelAlias": "kimi-code/k3",
                "turnStep": "0.1",
                "time": 1785312000000i64
            }),
            json!({
                "type": "context.append_loop_event",
                "agentId": "main",
                "event": {
                    "type": "tool.call",
                    "uuid": "tool-uuid-1",
                    "turnId": "0",
                    "step": 1,
                    "name": "Bash",
                    "args": {"command": "private command"}
                },
                "time": 1785312060000i64
            }),
            json!({
                "type": "context.append_loop_event",
                "agentId": "main",
                "event": {
                    "type": "step.end",
                    "uuid": "step-uuid-1",
                    "turnId": "0",
                    "step": 1,
                    "finishReason": "tool_use",
                    "usage": {
                        "inputOther": 2973,
                        "output": 150,
                        "inputCacheRead": 27648,
                        "inputCacheCreation": 0
                    },
                    "messageId": "chatcmpl-private"
                },
                "time": 1785312120000i64
            }),
            json!({
                "type": "context.append_loop_event",
                "agentId": "main",
                "event": {
                    "type": "step.end",
                    "uuid": "step-uuid-interrupted",
                    "turnId": "0",
                    "step": 2,
                    "finishReason": "interrupted"
                },
                "time": 1785312150000i64
            }),
            json!({"type": "usage.record", "private": true}),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(main_agents.join("wire.jsonl"), format!("{main_wire}\n")).unwrap();
        let sub_wire = [
            json!({
                "type": "llm.request",
                "agentId": "agent-0",
                "modelAlias": "kimi-code/k2.5",
                "turnStep": "0.1",
                "time": 1785312000000i64
            }),
            json!({
                "type": "context.append_loop_event",
                "agentId": "agent-0",
                "event": {
                    "type": "step.end",
                    "uuid": "step-uuid-2",
                    "turnId": "0",
                    "step": 1,
                    "finishReason": "stop",
                    "usage": {
                        "inputOther": 10,
                        "output": 20,
                        "inputCacheRead": 30,
                        "inputCacheCreation": 40
                    }
                },
                "time": 1785312180000i64
            }),
        ]
        .into_iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join("\n");
        fs::write(sub_agents.join("wire.jsonl"), format!("{sub_wire}\n")).unwrap();

        // When
        let data = collect(&root).unwrap();

        // Then
        assert_eq!(data.sessions.len(), 1);
        assert_eq!(data.sessions[0].source, Source::KimiCode);
        assert_eq!(data.sessions[0].session_id, "session_abc-123");
        assert_eq!(data.sessions[0].date, "2026-07-29");
        assert_eq!(data.sessions[0].project, "/tmp/kimi-project");
        assert_eq!(data.usage_events.len(), 2);
        let main_event = data
            .usage_events
            .iter()
            .find(|event| event.event_id == "step-uuid-1")
            .unwrap();
        assert_eq!(main_event.source, Source::KimiCode);
        assert_eq!(main_event.session_id, "session_abc-123");
        assert_eq!(main_event.date, "2026-07-29");
        assert_eq!(main_event.project, "/tmp/kimi-project");
        assert_eq!(main_event.model, "k3");
        assert_eq!(main_event.usage.input, 2973);
        assert_eq!(main_event.usage.cached_input, 27648);
        assert_eq!(main_event.usage.cache_creation_input, 0);
        assert_eq!(main_event.usage.cache_creation_input_5m, 0);
        assert_eq!(main_event.usage.cache_creation_input_1h, 0);
        assert_eq!(main_event.usage.output, 150);
        assert_eq!(main_event.usage.reasoning_output, 0);
        assert_eq!(main_event.usage.computed_total(), 30771);
        let sub_event = data
            .usage_events
            .iter()
            .find(|event| event.event_id == "step-uuid-2")
            .unwrap();
        assert_eq!(sub_event.model, "k2.5");
        assert_eq!(sub_event.usage.input, 10);
        assert_eq!(sub_event.usage.cached_input, 30);
        assert_eq!(sub_event.usage.cache_creation_input, 40);
        assert_eq!(sub_event.usage.output, 20);
        assert_eq!(sub_event.usage.computed_total(), 100);
        assert_eq!(data.tool_events.len(), 1);
        assert_eq!(data.tool_events[0].source, Source::KimiCode);
        assert_eq!(data.tool_events[0].event_id, "tool-uuid-1");
        assert_eq!(data.tool_events[0].tool, "Bash");
        assert_eq!(data.tool_events[0].date, "2026-07-29");
        assert!(data.warnings.is_empty());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn falls_back_to_session_dir_name_without_state_file() {
        // Given
        let counter = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "tokencheck-kimi-no-state-{}-{counter}",
            std::process::id()
        ));
        let agents = root.join("wd_project_deadbeef/session-orphan/agents/main");
        fs::create_dir_all(&agents).unwrap();
        let wire = json!({
            "type": "context.append_loop_event",
            "agentId": "main",
            "event": {
                "type": "step.end",
                "uuid": "step-uuid-orphan",
                "turnId": "0",
                "step": 1,
                "finishReason": "stop",
                "usage": {"inputOther": 1, "output": 1}
            },
            "time": 1785312120000i64
        });
        fs::write(agents.join("wire.jsonl"), format!("{wire}\n")).unwrap();

        // When
        let data = collect(&root).unwrap();

        // Then
        assert_eq!(data.sessions.len(), 1);
        assert_eq!(data.sessions[0].session_id, "session-orphan");
        assert_eq!(data.sessions[0].project, "unknown");
        assert_eq!(data.usage_events.len(), 1);
        assert_eq!(data.usage_events[0].model, "unknown");
        assert_eq!(data.usage_events[0].date, "2026-07-29");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_missing_root_as_warning() {
        // Given
        let root =
            std::env::temp_dir().join(format!("tokencheck-kimi-missing-{}", std::process::id()));

        // When
        let data = collect(&root).unwrap();

        // Then
        assert_eq!(
            data.warnings,
            vec![format!(
                "Kimi Code data directory not found: {}",
                root.display()
            )]
        );
    }
}
