JSON.stringify(
  (function () {
    const config = __SELECT_CONFIG__;

    __BROWSER_KERNEL__

    const element = resolveTargetElement(config);
    if (!element) {
      return {
        success: false,
        code: "target_detached",
        error: "Element is no longer present",
      };
    }

    if (element.tagName !== "SELECT") {
      return {
        success: false,
        code: "invalid_target",
        error: "Element is not a SELECT element",
      };
    }

    element.scrollIntoView({
      behavior: "auto",
      block: "center",
      inline: "center",
    });

    if (typeof element.focus === "function") {
      element.focus();
    }

    element.value = config.value;
    element.dispatchEvent(new Event("input", { bubbles: true }));
    element.dispatchEvent(new Event("change", { bubbles: true }));

    return {
      success: true,
      selectedValue: element.value,
      selectedText: element.options[element.selectedIndex]?.text ?? null,
    };
  })()
);
