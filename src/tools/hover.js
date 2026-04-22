JSON.stringify(
  (function () {
    const config = __HOVER_CONFIG__;

    __BROWSER_KERNEL__

    const element = resolveTargetElement(config);
    if (!element) {
      return {
        success: false,
        code: "target_detached",
        error: "Element is no longer present",
      };
    }

    element.scrollIntoView({
      behavior: "auto",
      block: "center",
      inline: "center",
    });

    const view = getDocumentView(element.ownerDocument);
    const rect = element.getBoundingClientRect();
    const event = new view.MouseEvent("mouseover", {
      view,
      bubbles: true,
      cancelable: true,
      clientX: rect.left + rect.width / 2,
      clientY: rect.top + rect.height / 2,
    });

    element.dispatchEvent(event);

    return {
      success: true,
      tagName: element.tagName,
      id: element.id,
      className: typeof element.className === "string" ? element.className : "",
    };
  })()
);
