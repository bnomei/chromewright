(() => {
  const config = __ACTIONABILITY_CONFIG__;
  const requested = new Set(config.predicates || []);

  __BROWSER_KERNEL__

  function summarizeElement(element) {
    if (!element) {
      return null;
    }

    const classes = typeof element.className === 'string'
      ? element.className.split(/\s+/).map((value) => value.trim()).filter(Boolean)
      : [];

    return {
      tag: element.tagName.toLowerCase(),
      id: element.id || null,
      classes
    };
  }

  function setPredicate(result, key, value) {
    if (requested.has(key)) {
      result[key] = value;
    }
  }

  const result = {
    present: false,
    visible: null,
    enabled: null,
    editable: null,
    stable: null,
    receives_events: null,
    in_viewport: null,
    unobscured_center: null,
    text_contains: null,
    value_equals: null,
    frame_depth: null,
    diagnostics: null
  };

  const match = resolveTargetMatch(config).match;
  if (!match || !match.element || !match.element.isConnected) {
    return JSON.stringify(result);
  }

  const element = match.element;
  const frameDepth = match.frame_depth || 0;
  const diagnostics = {};
  result.present = true;
  result.frame_depth = frameDepth;

  const needsLayout =
    requested.has('visible') ||
    requested.has('stable') ||
    requested.has('receives_events') ||
    requested.has('in_viewport') ||
    requested.has('unobscured_center');
  const needsDisabled = requested.has('enabled') || requested.has('editable');
  const needsText = requested.has('text_contains');
  const needsValue = requested.has('value_equals');

  let rect = null;
  let style = null;
  if (needsLayout) {
    rect = element.getBoundingClientRect();
    style = getDocumentView(element.ownerDocument).getComputedStyle(element);
    diagnostics.pointer_events = style.pointerEvents;
  }

  let disabled = null;
  if (needsDisabled) {
    disabled = Boolean(element.disabled) || element.getAttribute('aria-disabled') === 'true';
  }

  if (requested.has('visible')) {
    setPredicate(
      result,
      'visible',
      rect.width > 0 &&
        rect.height > 0 &&
        style.visibility !== 'hidden' &&
        style.display !== 'none'
    );
  }

  if (requested.has('enabled')) {
    setPredicate(result, 'enabled', !disabled);
  }

  if (requested.has('editable')) {
    setPredicate(
      result,
      'editable',
      !disabled && (
        element.matches('input, textarea, select') ||
        element.isContentEditable
      )
    );
  }

  if (requested.has('in_viewport')) {
    const view = getDocumentView(element.ownerDocument);
    setPredicate(
      result,
      'in_viewport',
      rect.bottom > 0 &&
        rect.right > 0 &&
        rect.top < view.innerHeight &&
        rect.left < view.innerWidth
    );
  }

  if (requested.has('stable')) {
    const nextRect = element.getBoundingClientRect();
    setPredicate(
      result,
      'stable',
      Math.abs(rect.x - nextRect.x) < 0.5 &&
        Math.abs(rect.y - nextRect.y) < 0.5 &&
        Math.abs(rect.width - nextRect.width) < 0.5 &&
        Math.abs(rect.height - nextRect.height) < 0.5
    );
  }

  if (requested.has('receives_events') || requested.has('unobscured_center')) {
    const centerX = rect.left + rect.width / 2;
    const centerY = rect.top + rect.height / 2;
    const hitTarget = style.pointerEvents === 'none'
      ? null
      : element.ownerDocument.elementFromPoint(centerX, centerY);
    const receivesEvents = Boolean(hitTarget) && (
      hitTarget === element ||
      element.contains(hitTarget) ||
      hitTarget.contains(element)
    );

    diagnostics.hit_target = summarizeElement(hitTarget);
    setPredicate(result, 'receives_events', receivesEvents);
    setPredicate(result, 'unobscured_center', receivesEvents);
  }

  if (needsText) {
    const text = (element.innerText || element.textContent || '').trim();
    diagnostics.text_length = text.length;
    setPredicate(result, 'text_contains', text.includes(config.text || ''));
  }

  if (needsValue) {
    const value = ('value' in element) ? element.value : null;
    diagnostics.has_value = value !== null;
    setPredicate(result, 'value_equals', value === config.value);
  }

  result.diagnostics = Object.keys(diagnostics).length > 0 ? diagnostics : null;
  return JSON.stringify(result);
})()
