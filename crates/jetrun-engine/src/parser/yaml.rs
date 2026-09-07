use jetrun_common::error::JetrunError;
use jetrun_common::models::PipelineConfig;

/// Parse a YAML string into a validated PipelineConfig
pub fn parse_pipeline(yaml: &str) -> Result<PipelineConfig, JetrunError> {
    let config: PipelineConfig = serde_yaml::from_str(yaml)?;
    validate_config(&config)?;
    Ok(config)
}

fn validate_config(config: &PipelineConfig) -> Result<(), JetrunError> {
    if config.name.is_empty() {
        return Err(JetrunError::InvalidConfig(
            "Pipeline name cannot be empty".into(),
        ));
    }

    if config.stages.is_empty() {
        return Err(JetrunError::InvalidConfig(
            "Pipeline must have at least one stage".into(),
        ));
    }

    let stage_names: Vec<&str> = config.stages.iter().map(|s| s.name.as_str()).collect();

    // Check for duplicate stage names
    let mut seen = std::collections::HashSet::new();
    for name in &stage_names {
        if !seen.insert(name) {
            return Err(JetrunError::InvalidConfig(format!(
                "Duplicate stage name: '{}'",
                name
            )));
        }
    }

    // Validate depends_on references exist
    for stage in &config.stages {
        for dep in &stage.depends_on {
            if !stage_names.contains(&dep.as_str()) {
                return Err(JetrunError::InvalidConfig(format!(
                    "Stage '{}' depends on unknown stage '{}'",
                    stage.name, dep
                )));
            }
        }

        if stage.steps.is_empty() {
            return Err(JetrunError::InvalidConfig(format!(
                "Stage '{}' must have at least one step",
                stage.name
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_pipeline() {
        let yaml = r#"
name: "Test Pipeline"
on:
  push:
    branches: ["main"]
stages:
  - name: build
    steps:
      - name: compile
        run: cargo build
"#;
        let config = parse_pipeline(yaml).unwrap();
        assert_eq!(config.name, "Test Pipeline");
        assert_eq!(config.stages.len(), 1);
        assert_eq!(config.stages[0].steps[0].run, "cargo build");
    }

    #[test]
    fn test_reject_empty_name() {
        let yaml = r#"
name: ""
on:
  webhook: true
stages:
  - name: build
    steps:
      - name: compile
        run: cargo build
"#;
        assert!(parse_pipeline(yaml).is_err());
    }

    #[test]
    fn test_reject_invalid_depends_on() {
        let yaml = r#"
name: "Test"
on:
  webhook: true
stages:
  - name: build
    depends_on: ["nonexistent"]
    steps:
      - name: compile
        run: cargo build
"#;
        assert!(parse_pipeline(yaml).is_err());
    }
}
