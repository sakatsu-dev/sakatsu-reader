//! The text behind "Copy details for a bug report".
//!
//! **Built from a fixed list of facts, never from the settings.** Serialising the settings and
//! deleting the private parts afterwards fails open: the next setting somebody adds — a deck name,
//! an endpoint, a folder — is in the report until somebody remembers to delete it. So the report
//! is assembled from [`ReportInput`], whose fields are the whole of what it can say, and every
//! string in it passes through [`safe_label`] on the way out, so not even a field filled carelessly
//! can carry a path or an address through.
//!
//! What it never contains: passage text, translations, screenshots, the clipboard, vocabulary,
//! deck names, account names, folders, endpoint addresses, credentials, raw log lines, error
//! messages, and the request identities the status carries.

use crate::runtime_status::{
    safe_label, Acceleration, CacheUse, EngineRole, EngineState, EngineStatus, Hosting,
    OperationResult, Outcome, RuntimeStatus,
};

/// The operating system, as three short facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OsFacts {
    pub family: String,
    pub version: String,
    pub arch: String,
}

/// Where an installable resource stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceState {
    Ready,
    NotInstalled,
    Downloading,
    Paused,
    NeedsAttention,
    Other,
}

impl ResourceState {
    fn code(self) -> &'static str {
        match self {
            ResourceState::Ready => "ready",
            ResourceState::NotInstalled => "not installed",
            ResourceState::Downloading => "downloading",
            ResourceState::Paused => "paused",
            ResourceState::NeedsAttention => "needs attention",
            ResourceState::Other => "other",
        }
    }
}

/// One installable resource: its catalog id, its installed version, and where it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceFact {
    pub id: String,
    pub version: Option<String>,
    pub state: ResourceState,
}

/// Everything the report may say. Adding a fact means adding a field here, on purpose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportInput {
    pub version: String,
    pub release_build: bool,
    pub os: OsFacts,
    pub status: RuntimeStatus,
    pub resources: Vec<ResourceFact>,
    /// The setup phase, as its code, when setup has run.
    pub setup: Option<String>,
    pub logging: bool,
}

/// The labels of the report's lines, in order. A test holds the report to exactly these.
pub const REPORT_KEYS: [&str; 12] = [
    "Version",
    "System",
    "Recognition",
    "Analysis",
    "Translation",
    "Last recognition",
    "Last analysis",
    "Last translation",
    "Resources",
    "Setup",
    "Logging",
    "Log file",
];

/// The report, as the text the reader previews and copies.
pub fn report(input: &ReportInput) -> String {
    let mut lines = vec!["Sakatsu Reader diagnostic report".to_string()];
    let mut line = |key: &str, value: String| lines.push(format!("{key}: {value}"));

    line(
        "Version",
        format!(
            "{} ({} build)",
            safe_label(&input.version),
            if input.release_build {
                "release"
            } else {
                "development"
            }
        ),
    );
    line(
        "System",
        format!(
            "{} {}, {}",
            safe_label(&input.os.family),
            safe_label(&input.os.version),
            safe_label(&input.os.arch)
        ),
    );
    for (key, role) in [
        ("Recognition", EngineRole::Recognition),
        ("Analysis", EngineRole::Analysis),
        ("Translation", EngineRole::Translation),
    ] {
        let engine = input
            .status
            .engines
            .iter()
            .find(|engine| engine.role == role);
        line(
            key,
            engine.map_or_else(|| "not reported".to_string(), engine_line),
        );
    }
    line(
        "Last recognition",
        result_line(input.status.recognition.as_ref()),
    );
    line("Last analysis", result_line(input.status.analysis.as_ref()));
    line(
        "Last translation",
        result_line(input.status.translation.as_ref()),
    );
    line(
        "Resources",
        if input.resources.is_empty() {
            "none reported".to_string()
        } else {
            input
                .resources
                .iter()
                .map(|resource| {
                    let version = resource
                        .version
                        .as_deref()
                        .map(safe_label)
                        .filter(|version| !version.is_empty())
                        .map(|version| format!(" {version}"))
                        .unwrap_or_default();
                    format!(
                        "{}{} {}",
                        safe_label(&resource.id),
                        version,
                        resource.state.code()
                    )
                })
                .collect::<Vec<_>>()
                .join("; ")
        },
    );
    line(
        "Setup",
        input
            .setup
            .as_deref()
            .map(safe_label)
            .unwrap_or_else(|| "not run".to_string()),
    );
    line(
        "Logging",
        if input.logging { "on" } else { "off" }.to_string(),
    );
    line("Log file", "sakatsu.log".to_string());
    lines.join("\n")
}

fn engine_line(engine: &EngineStatus) -> String {
    let loaded = engine
        .loaded
        .as_deref()
        .map(|name| format!(", loaded {}", safe_label(name)))
        .unwrap_or_default();
    format!(
        "configured {}{loaded}; {}; {}; {}",
        safe_label(&engine.configured),
        hosting_phrase(engine.hosting),
        state_phrase(engine.state),
        acceleration_phrase(&engine.acceleration)
    )
}

fn result_line(result: Option<&OperationResult>) -> String {
    let Some(result) = result else {
        return "none this session".to_string();
    };
    let outcome = match result.outcome {
        Outcome::Succeeded => "succeeded".to_string(),
        Outcome::Failed { code } => format!("failed ({})", code.code()),
    };
    let cache = match result.cache {
        CacheUse::Fresh => "fresh",
        CacheUse::Cached => "from cache",
        CacheUse::Mixed => "partly from cache",
    };
    let fallback = result
        .fallback
        .map(|reason| format!(", fallback {}", safe_label(&format!("{reason:?}"))))
        .unwrap_or_default();
    format!(
        "{} {outcome}, {cache}{fallback}; {}",
        safe_label(&result.engine),
        acceleration_phrase(&result.acceleration)
    )
}

fn hosting_phrase(hosting: Hosting) -> &'static str {
    match hosting {
        Hosting::InProcess => "in the app",
        Hosting::ManagedLocal => "local service the app runs",
        Hosting::Endpoint { external: false } => "local endpoint",
        Hosting::Endpoint { external: true } => "external endpoint",
    }
}

fn state_phrase(state: EngineState) -> &'static str {
    match state {
        EngineState::NotInstalled => "not installed",
        EngineState::NotLoaded => "not loaded",
        EngineState::Loading => "loading",
        EngineState::Ready => "ready",
    }
}

/// The acceleration phrase, with the device label when llama.cpp named one.
fn acceleration_phrase(acceleration: &Acceleration) -> String {
    match acceleration {
        Acceleration::GpuLayers {
            device: Some(device),
            ..
        } => {
            format!("{} on {}", acceleration.phrase(), safe_label(device))
        }
        other => other.phrase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FallbackReason;
    use crate::request::{RequestIdentity, WorkPriority};
    use crate::runtime_status::FailureCode;

    /// Strings no report may contain, whichever free-text field they are pushed into.
    const SECRETS: [&str; 6] = [
        "C:\\Users\\someone",
        "/home/someone",
        "http://192.168.1.10:8140",
        "deck::Private",
        "\u{79d8}\u{5bc6}",
        "token=abc123",
    ];

    /// The webview's request owner. The status carries it so the interface can match results to
    /// requests; the report never prints a request identity at all.
    const OWNER: &str = "00000000-1111-2222-3333-444444444444";

    fn hostile() -> ReportInput {
        let poisoned = SECRETS.join(" ");
        let request = RequestIdentity {
            owner: OWNER.to_string(),
            operation: "capture".into(),
            generation: 7,
            priority: WorkPriority::Foreground,
        };
        let result = |role| OperationResult {
            role,
            request: Some(request.clone()),
            engine: poisoned.clone(),
            fallback: Some(FallbackReason::Unavailable),
            cache: CacheUse::Mixed,
            outcome: Outcome::Failed {
                code: FailureCode::EngineFailed,
            },
            acceleration: Acceleration::GpuLayers {
                offloaded: 1,
                total: 2,
                device: Some(poisoned.clone()),
            },
        };
        ReportInput {
            version: poisoned.clone(),
            release_build: false,
            os: OsFacts {
                family: poisoned.clone(),
                version: poisoned.clone(),
                arch: poisoned.clone(),
            },
            status: RuntimeStatus {
                engines: vec![EngineStatus {
                    role: EngineRole::Recognition,
                    configured: poisoned.clone(),
                    loaded: Some(poisoned.clone()),
                    hosting: Hosting::Endpoint { external: true },
                    state: EngineState::Ready,
                    acceleration: Acceleration::Unknown,
                }],
                recognition: Some(result(EngineRole::Recognition)),
                analysis: Some(result(EngineRole::Analysis)),
                translation: Some(result(EngineRole::Translation)),
            },
            resources: vec![ResourceFact {
                id: poisoned.clone(),
                version: Some(poisoned.clone()),
                state: ResourceState::NeedsAttention,
            }],
            setup: Some(poisoned),
            logging: true,
        }
    }

    #[test]
    fn nothing_private_survives_even_in_fields_filled_carelessly() {
        let text = report(&hostile());
        for secret in SECRETS {
            assert!(
                !text.contains(secret),
                "the report carries {secret:?}:\n{text}"
            );
        }
        assert!(
            !text.contains(OWNER),
            "the report carries a request identity:\n{text}"
        );
        for fragment in ["C:\\", ":\\", "://", "/home", "::"] {
            assert!(
                !text.contains(fragment),
                "the report carries {fragment:?}:\n{text}"
            );
        }
    }

    #[test]
    fn the_report_has_exactly_its_fixed_lines() {
        let text = report(&hostile());
        let keys: Vec<&str> = text
            .lines()
            .skip(1)
            .map(|line| line.split_once(": ").expect("every line is key: value").0)
            .collect();
        assert_eq!(keys, REPORT_KEYS);
    }

    #[test]
    fn an_ordinary_session_reads_plainly() {
        let input = ReportInput {
            version: "1.0.0".into(),
            release_build: true,
            os: OsFacts {
                family: "windows".into(),
                version: "10.0.26200".into(),
                arch: "x86_64".into(),
            },
            status: RuntimeStatus {
                engines: vec![EngineStatus {
                    role: EngineRole::Translation,
                    configured: "fugumt".into(),
                    loaded: Some("fugumt".into()),
                    hosting: Hosting::InProcess,
                    state: EngineState::Ready,
                    acceleration: Acceleration::Cpu,
                }],
                recognition: None,
                analysis: None,
                translation: None,
            },
            resources: vec![ResourceFact {
                id: "translation".into(),
                version: Some("1".into()),
                state: ResourceState::Ready,
            }],
            setup: Some("complete".into()),
            logging: false,
        };
        let text = report(&input);
        assert!(text.contains("Version: 1.0.0 (release build)"), "{text}");
        assert!(
            text.contains("System: windows 10.0.26200, x86_64"),
            "{text}"
        );
        assert!(
            text.contains("Translation: configured fugumt, loaded fugumt; in the app; ready; CPU"),
            "{text}"
        );
        assert!(text.contains("Recognition: not reported"), "{text}");
        assert!(
            text.contains("Last recognition: none this session"),
            "{text}"
        );
        assert!(text.contains("Resources: translation 1 ready"), "{text}");
        assert!(text.contains("Logging: off"), "{text}");
    }

    /// A resource with no version reads as its id and state, without an empty gap between them.
    #[test]
    fn an_unversioned_resource_has_no_gap() {
        let mut input = hostile();
        input.resources = vec![ResourceFact {
            id: "ichiran".into(),
            version: Some(String::new()),
            state: ResourceState::Ready,
        }];
        assert!(
            report(&input).contains("Resources: ichiran ready"),
            "{}",
            report(&input)
        );
    }
}
