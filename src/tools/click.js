// Click interaction script compiled with browser-kernel and target config.
(() => {
  const config = __CLICK_CONFIG__;

  __BROWSER_KERNEL__

  const element = resolveTargetElement(config);
  if (!element) {
    return JSON.stringify({
      success: false,
      code: 'target_detached',
      error: 'Element is no longer present'
    });
  }

  if (typeof element.scrollIntoView === 'function') {
    element.scrollIntoView({
      behavior: 'auto',
      block: 'center',
      inline: 'center'
    });
  }

  element.click();

  return JSON.stringify({ success: true });
})()
