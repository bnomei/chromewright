(() => {
  const config = __REVEAL_CONFIG__;

  __BROWSER_KERNEL__

  // Resolve through the shared kernel so reveal scrolls the same DOM node that
  // inspect_node resolves for the same target, honoring target_index and
  // cross-frame (iframe) lookup. A plain document.querySelector would ignore
  // both and could scroll a colliding or main-document element instead.
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
