use rootcause::{Result, prelude::*};

const PATCHBAY_RUN_AGENT_PREFIX: &str = "patchbay-run-";

pub(crate) fn patchbay_run_agent_id(run_id: i64) -> String {
    debug_assert!(run_id > 0, "Patchbay run ids must be positive");
    format!("{PATCHBAY_RUN_AGENT_PREFIX}{run_id}")
}

pub(crate) fn parse_patchbay_run_agent_id(agent_id: &str) -> Option<i64> {
    let id = agent_id.strip_prefix(PATCHBAY_RUN_AGENT_PREFIX)?;
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    let run_id = id.parse::<i64>().ok()?;
    (run_id > 0).then_some(run_id)
}

pub(crate) fn validate_agent_id(agent_id: &str) -> Result<()> {
    if agent_id.trim().is_empty() {
        bail!("agent id cannot be empty");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patchbay_run_agent_ids_round_trip_valid_positive_run_ids() {
        let agent_id = patchbay_run_agent_id(42);

        assert_eq!(agent_id, "patchbay-run-42");
        assert_eq!(parse_patchbay_run_agent_id(&agent_id), Some(42));
    }

    #[test]
    fn patchbay_run_agent_id_parser_rejects_non_canonical_ids() {
        for agent_id in [
            "",
            "codex",
            "patchbay-run-",
            "patchbay-run-0",
            "patchbay-run-+60",
            "patchbay-run- 60",
            "patchbay-run-abc",
        ] {
            assert_eq!(parse_patchbay_run_agent_id(agent_id), None, "{agent_id}");
        }
    }

    #[test]
    fn agent_id_validation_rejects_blank_ids() {
        assert!(validate_agent_id("agent-a").is_ok());
        assert!(validate_agent_id(" ").is_err());
    }
}
