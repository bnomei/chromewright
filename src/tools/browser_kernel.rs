use serde_json::Value;
use std::sync::OnceLock;

const BROWSER_KERNEL_JS: &str = include_str!("browser_kernel.js");

pub(crate) struct BrowserKernelTemplateShell {
    prefix: String,
    suffix: String,
}

impl BrowserKernelTemplateShell {
    fn compile(template: &'static str, config_placeholder: &'static str) -> Self {
        let expanded = template.replace("__BROWSER_KERNEL__", BROWSER_KERNEL_JS);
        let mut parts = expanded.split(config_placeholder);
        let prefix = parts
            .next()
            .expect("expanded browser-kernel template should have a prefix");
        let suffix = parts
            .next()
            .expect("browser-kernel template must contain exactly one config placeholder");
        assert!(
            parts.next().is_none(),
            "browser-kernel template must contain exactly one config placeholder"
        );

        Self {
            prefix: prefix.to_string(),
            suffix: suffix.to_string(),
        }
    }

    fn render(&self, config: &Value) -> String {
        let config_json = config.to_string();
        let mut rendered =
            String::with_capacity(self.prefix.len() + config_json.len() + self.suffix.len());
        rendered.push_str(&self.prefix);
        rendered.push_str(&config_json);
        rendered.push_str(&self.suffix);
        rendered
    }
}

pub(crate) fn render_browser_kernel_script(
    shell_cache: &OnceLock<BrowserKernelTemplateShell>,
    template: &'static str,
    config_placeholder: &'static str,
    config: &Value,
) -> String {
    shell_cache
        .get_or_init(|| BrowserKernelTemplateShell::compile(template, config_placeholder))
        .render(config)
}

#[cfg(test)]
mod tests {
    use super::BrowserKernelTemplateShell;
    use super::render_browser_kernel_script;
    use std::sync::OnceLock;

    #[test]
    fn test_render_browser_kernel_script_injects_kernel_and_config() {
        let shell = OnceLock::new();
        let rendered = render_browser_kernel_script(
            &shell,
            "const config = __CONFIG__;\n__BROWSER_KERNEL__\nreturn config;",
            "__CONFIG__",
            &serde_json::json!({ "selector": "#save" }),
        );

        assert!(rendered.contains(r##"const config = {"selector":"#save"};"##));
        assert!(rendered.contains("function resolveTargetMatch(config, options)"));
        assert!(!rendered.contains("__BROWSER_KERNEL__"));
        assert!(!rendered.contains("__CONFIG__"));
    }

    #[test]
    fn test_browser_kernel_template_shell_renders_equivalent_output() {
        let shell = BrowserKernelTemplateShell::compile(
            "const config = __CONFIG__;\n__BROWSER_KERNEL__\nreturn config.selector;",
            "__CONFIG__",
        );

        let rendered = shell.render(&serde_json::json!({ "selector": "#save" }));

        assert!(rendered.starts_with(r##"const config = {"selector":"#save"};"##));
        assert!(rendered.contains("function resolveTargetMatch(config, options)"));
        assert!(rendered.ends_with("\nreturn config.selector;"));
    }
}
