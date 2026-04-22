(() => {
  const config = __SCROLL_TARGET_CONFIG__;

  __BROWSER_KERNEL__

  const element = resolveTargetElement(config);
  if (!element) {
    return JSON.stringify({ scrolled: false });
  }

  if (typeof element.scrollIntoView === 'function') {
    element.scrollIntoView({
      behavior: 'auto',
      block: 'center',
      inline: 'center'
    });
  }

  return JSON.stringify({ scrolled: true });
})()
