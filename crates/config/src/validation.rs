use crate::error::ConfigError;

pub(crate) fn validate_plugin_name(value: &str) -> Result<(), ConfigError> {
    let valid_alias = !value.starts_with('@') && valid_component(value);
    let valid_scoped = value.starts_with('@')
        && value
            .trim_start_matches('@')
            .split_once('/')
            .is_some_and(|(scope, name)| valid_component(scope) && valid_component(name));
    if valid_alias || valid_scoped {
        Ok(())
    } else {
        Err(ConfigError::InvalidPluginName(value.to_owned()))
    }
}

pub(crate) fn validate_plugin_tool_name(value: &str) -> Result<(), ConfigError> {
    if valid_component(value) {
        Ok(())
    } else {
        Err(ConfigError::InvalidPluginToolName(value.to_owned()))
    }
}

pub(crate) fn valid_component(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '_' | '-')
        })
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        && value
            .chars()
            .last()
            .is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
}

pub(crate) fn validate_numeric_selector(selector: &str) -> Result<(), ()> {
    if selector.eq_ignore_ascii_case("latest") {
        return Ok(());
    }
    let components = selector.split('.').collect::<Vec<_>>();
    if components.is_empty() || components.len() > 3 {
        return Err(());
    }
    let mut wildcard_seen = false;
    for component in components {
        if component.eq_ignore_ascii_case("x") || component == "*" {
            wildcard_seen = true;
        } else if wildcard_seen
            || component.is_empty()
            || !component
                .chars()
                .all(|character| character.is_ascii_digit())
        {
            return Err(());
        }
    }
    Ok(())
}
