(() => {
  const config = __INSPECT_CONFIG__;

  __BROWSER_KERNEL__

  function normalizeWhiteSpace(text) {
    return String(text || '').replace(/\s+/g, ' ').trim();
  }

  function getElementAccessibleName(element) {
    const doc = element.ownerDocument;

    const ariaLabel = element.getAttribute('aria-label');
    if (ariaLabel) {
      return ariaLabel;
    }

    const labelledBy = element.getAttribute('aria-labelledby');
    if (labelledBy) {
      const ids = labelledBy.split(/\s+/);
      const texts = ids
        .map((id) => {
          const labelled = doc.getElementById(id);
          return labelled ? labelled.textContent : '';
        })
        .filter(Boolean);
      if (texts.length > 0) {
        return texts.join(' ');
      }
    }

    if (['INPUT', 'TEXTAREA', 'SELECT'].includes(element.tagName)) {
      const id = element.id;
      if (id) {
        const label = doc.querySelector('label[for="' + id + '"]');
        if (label) {
          return label.textContent || '';
        }
      }

      const parentLabel = element.closest('label');
      if (parentLabel) {
        return parentLabel.textContent || '';
      }
    }

    if (element.tagName === 'IMG') {
      return element.getAttribute('alt') || '';
    }

    const title = element.getAttribute('title');
    if (title) {
      return title;
    }

    if (element.tagName === 'INPUT' || element.tagName === 'TEXTAREA') {
      const placeholder = element.getAttribute('placeholder');
      if (placeholder) {
        return placeholder;
      }
    }

    if (element.tagName === 'A' || element.tagName === 'BUTTON') {
      const text = element.textContent || '';
      if (text.trim()) {
        return text.trim();
      }
    }

    return '';
  }

  function getAriaChecked(element) {
    const checked = element.getAttribute('aria-checked');
    if (checked === 'true') return true;
    if (checked === 'false') return false;
    if (checked === 'mixed') return 'mixed';

    if (element.tagName === 'INPUT' && (element.type === 'checkbox' || element.type === 'radio')) {
      return element.checked;
    }

    return null;
  }

  function getAriaDisabled(element) {
    const disabled = element.getAttribute('aria-disabled');
    if (disabled === 'true') return true;

    if (element.disabled !== undefined) {
      return Boolean(element.disabled);
    }

    return null;
  }

  function getAriaExpanded(element) {
    const expanded = element.getAttribute('aria-expanded');
    if (expanded === 'true') return true;
    if (expanded === 'false') return false;
    return null;
  }

  function getAriaPressed(element) {
    const pressed = element.getAttribute('aria-pressed');
    if (pressed === 'true') return true;
    if (pressed === 'false') return false;
    if (pressed === 'mixed') return 'mixed';
    return null;
  }

  function getAriaSelected(element) {
    const selected = element.getAttribute('aria-selected');
    if (selected === 'true') return true;
    if (selected === 'false') return false;

    if (element.tagName === 'OPTION') {
      return Boolean(element.selected);
    }

    return null;
  }

  function boundString(value, maxChars) {
    const text = String(value || '');
    return {
      value: text.slice(0, maxChars),
      truncated: text.length > maxChars,
      total_chars: text.length
    };
  }

  function boundMap(entries, maxEntries, maxValueChars) {
    const values = {};
    let truncated = false;

    entries.slice(0, maxEntries).forEach(([key, rawValue]) => {
      const value = String(rawValue || '');
      if (value.length > maxValueChars) {
        truncated = true;
      }
      values[key] = value.slice(0, maxValueChars);
    });

    if (entries.length > maxEntries) {
      truncated = true;
    }

    return {
      values,
      truncated,
      total_entries: entries.length
    };
  }

  function buildSections(element) {
    if (config.detail !== 'full') {
      return null;
    }

    const styleNames = Array.isArray(config.style_names) ? config.style_names : [];
    const attributes = Array.from(element.attributes || []).map((attribute) => [attribute.name, attribute.value]);
    const styles = styleNames.map((name) => [name, getDocumentView(element.ownerDocument).getComputedStyle(element).getPropertyValue(name).trim()]);

    return {
      text: boundString(element.innerText || element.textContent || '', 2000),
      html: boundString(element.outerHTML || '', 4000),
      attributes: boundMap(attributes, 24, 400),
      styles: boundMap(styles, styleNames.length || 12, 200)
    };
  }

  function buildBoundary(element) {
    if (element.tagName !== 'IFRAME') {
      return null;
    }

    const boundary = {
      kind: 'iframe',
      status: 'unavailable',
      available: false,
      url: null
    };

    try {
      const frameDoc = element.contentDocument;
      const frameWindow = element.contentWindow;
      if (!frameDoc || !frameWindow) {
        return boundary;
      }

      boundary.status = 'expanded';
      boundary.available = true;
      boundary.url = frameWindow.location.href;
      return boundary;
    } catch (error) {
      boundary.status = 'cross_origin';
      return boundary;
    }
  }

  function buildSelector(element) {
    if (!element || !element.ownerDocument) {
      return null;
    }

    const doc = element.ownerDocument;
    if (element.id) {
      return '#' + escapeCssIdentifier(element.id);
    }

    const path = [];
    let current = element;

    while (current && current !== doc.body) {
      let selector = current.tagName.toLowerCase();

      if (current.className && typeof current.className === 'string') {
        const classes = current.className.trim().split(/\s+/).filter(Boolean);
        if (classes.length > 0) {
          selector += '.' + escapeCssIdentifier(classes[0]);
        }
      }

      const parent = current.parentElement;
      if (parent) {
        const siblings = Array.from(parent.children);
        const siblingIndex = siblings.indexOf(current);
        if (siblings.filter((sibling) => sibling.tagName === current.tagName).length > 1) {
          selector += ':nth-child(' + (siblingIndex + 1) + ')';
        }
      }

      path.unshift(selector);
      current = current.parentElement;
    }

    return path.join(' > ') || null;
  }

  function inspectElement(element, frameDepth, actionableIndex) {
    const view = getDocumentView(element.ownerDocument);
    const box = computeBox(element);
    const rect = box.rect;
    const role = getAriaRole(element);
    const name = normalizeWhiteSpace(getElementAccessibleName(element) || '');
    const insideShadowRoot = typeof ShadowRoot !== 'undefined' && element.getRootNode() instanceof ShadowRoot;
    const classes = typeof element.className === 'string'
      ? element.className.split(/\s+/).map((namePart) => namePart.trim()).filter(Boolean)
      : [];
    const disabled = getAriaDisabled(element);

    return {
      success: true,
      identity: {
        tag: element.tagName.toLowerCase(),
        id: element.id || null,
        classes
      },
      accessibility: {
        role,
        name,
        active: element.ownerDocument.activeElement === element,
        checked: getAriaChecked(element),
        disabled,
        expanded: getAriaExpanded(element),
        pressed: getAriaPressed(element),
        selected: getAriaSelected(element)
      },
      form_state: {
        value: 'value' in element ? String(element.value || '') : null,
        placeholder: element.getAttribute('placeholder'),
        readonly: 'readOnly' in element ? Boolean(element.readOnly) : null,
        disabled
      },
      layout: {
        bounding_box: {
          x: rect.x,
          y: rect.y,
          width: rect.width,
          height: rect.height
        },
        visible: box.visible,
        visible_in_viewport:
          rect.bottom > 0 &&
          rect.right > 0 &&
          rect.top < view.innerHeight &&
          rect.left < view.innerWidth,
        receives_pointer_events: receivesPointerEvents(element),
        pointer_events: box.pointerEvents,
        cursor: box.cursor || null
      },
      context: {
        document_url: element.ownerDocument.location ? element.ownerDocument.location.href : document.location.href,
        frame_depth: frameDepth,
        inside_shadow_root: insideShadowRoot
      },
      actionable_index: actionableIndex,
      resolved_selector: buildSelector(element),
      boundary: buildBoundary(element),
      sections: buildSections(element)
    };
  }

  try {
    const resolved = resolveTargetMatch(config, { collectBoundaries: true });
    const match = resolved.match;
    const selectorSearch = resolved.selector_search;

    if (!match || !match.element) {
      if (selectorSearch && selectorSearch.boundaries.length > 0) {
        return JSON.stringify({
          success: false,
          code: 'cross_origin_frame_boundary',
          error: 'Element could not be inspected because matching content may be inside an unavailable or cross-origin iframe',
          boundaries: selectorSearch.boundaries
        });
      }

      return JSON.stringify({
        success: false,
        code: 'target_not_found',
        error: 'Element not found for inspection'
      });
    }

    const actionableIndex =
      (match.frame_depth || 0) === 0
        ? findActionableIndexForElement(match.element)
        : typeof config.target_index === 'number'
          ? config.target_index
          : null;

    return JSON.stringify(inspectElement(match.element, match.frame_depth || 0, actionableIndex));
  } catch (error) {
    return JSON.stringify({
      success: false,
      code: 'inspect_failed',
      error: error && error.message ? error.message : String(error)
    });
  }
})()
