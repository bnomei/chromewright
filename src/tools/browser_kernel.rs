use serde_json::Value;

const BROWSER_KERNEL_JS: &str = include_str!("browser_kernel.js");

pub(crate) fn render_browser_kernel_script(
    template: &str,
    config_placeholder: &str,
    config: &Value,
) -> String {
    template
        .replace("__BROWSER_KERNEL__", BROWSER_KERNEL_JS)
        .replace(config_placeholder, &config.to_string())
}

#[cfg(test)]
mod tests {
    use super::render_browser_kernel_script;

    #[test]
    fn test_render_browser_kernel_script_injects_kernel_and_config() {
        let rendered = render_browser_kernel_script(
            "const config = __CONFIG__;\n__BROWSER_KERNEL__\nreturn config;",
            "__CONFIG__",
            &serde_json::json!({ "selector": "#save" }),
        );

        assert!(rendered.contains(r##"const config = {"selector":"#save"};"##));
        assert!(rendered.contains("function resolveTargetMatch(config, options)"));
        assert!(!rendered.contains("__BROWSER_KERNEL__"));
        assert!(!rendered.contains("__CONFIG__"));
    }
}
