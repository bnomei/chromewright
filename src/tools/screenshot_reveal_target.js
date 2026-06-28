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

  const topViewBefore = getTopLevelViewForElement(element);
  const scrollYBefore = topViewBefore.scrollY || 0;
  let current = element;
  while (current) {
    if (typeof current.scrollIntoView === 'function') {
      current.scrollIntoView({
        block: 'center',
        inline: 'center',
        behavior: 'auto'
      });
    }

    const currentView = getDocumentView(current.ownerDocument);
    const frameElement = getFrameElementForView(currentView);
    current = frameElement && frameElement.isConnected ? frameElement : null;
  }

  const topViewAfter = getTopLevelViewForElement(element);
  const rect = computeViewportRect(element);
  return JSON.stringify({
    success: true,
    scroll_y_before: scrollYBefore,
    scroll_y_after: topViewAfter.scrollY || 0,
    visible_in_viewport:
      rect.bottom > 0 &&
      rect.right > 0 &&
      rect.top < topViewAfter.innerHeight &&
      rect.left < topViewAfter.innerWidth
  });
})()
