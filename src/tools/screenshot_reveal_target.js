// Scrolls an offscreen target into view before element or region screenshots.
(() => {
  const config = __REVEAL_CONFIG__;

  __BROWSER_KERNEL__

  const element = resolveTargetElement(config);
  if (!element) {
    return JSON.stringify({
      success: false,
      code: 'target_not_found',
      error: 'Element not found for screenshot reveal'
    });
  }

  const scrollYBefore = window.scrollY || 0;
  if (typeof element.scrollIntoView === 'function') {
    element.scrollIntoView({
      block: 'center',
      inline: 'center',
      behavior: 'auto'
    });
  }

  const rect = element.getBoundingClientRect();
  return JSON.stringify({
    success: true,
    scroll_y_before: scrollYBefore,
    scroll_y_after: window.scrollY || 0,
    visible_in_viewport:
      rect.bottom > 0 &&
      rect.right > 0 &&
      rect.top < window.innerHeight &&
      rect.left < window.innerWidth
  });
})()
