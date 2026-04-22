(() => {
  const config = __INPUT_CONFIG__;

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

  if (typeof element.focus === 'function') {
    element.focus();
  }

  const dispatchInput = () => {
    element.dispatchEvent(new Event('input', { bubbles: true }));
    element.dispatchEvent(new Event('change', { bubbles: true }));
  };

  if ('value' in element) {
    const nextValue = config.clear ? config.text : `${element.value ?? ''}${config.text}`;
    element.value = nextValue;
    dispatchInput();
    return JSON.stringify({
      success: true,
      value: nextValue
    });
  }

  if (element.isContentEditable) {
    const nextValue = config.clear ? config.text : `${element.textContent ?? ''}${config.text}`;
    element.textContent = nextValue;
    dispatchInput();
    return JSON.stringify({
      success: true,
      value: nextValue
    });
  }

  return JSON.stringify({
    success: false,
    code: 'invalid_target',
    error: 'Element does not accept text input'
  });
})()
