(() => {
  const config = __TARGET_EXISTS_CONFIG__;

  __BROWSER_KERNEL__

  return JSON.stringify({
    present: Boolean(config.selector && selectorExistsAcrossScopes(config.selector))
  });
})()
