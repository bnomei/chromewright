(() => {
  const config = __INSPECT_CONFIG__;

  function getDocumentView(doc) {
    return doc.defaultView || window;
  }

  function normalizeWhiteSpace(text) {
    return String(text || '').replace(/\s+/g, ' ').trim();
  }

  function isElementHiddenForAria(element) {
    const tagName = element.tagName;
    if (['STYLE', 'SCRIPT', 'NOSCRIPT', 'TEMPLATE'].includes(tagName)) {
      return true;
    }

    const style = getDocumentView(element.ownerDocument).getComputedStyle(element);
    if (style.visibility !== 'visible' || style.display === 'none') {
      return true;
    }

    if (element.getAttribute('aria-hidden') === 'true') {
      return true;
    }

    return false;
  }

  function isElementVisible(element) {
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  }

  function computeBox(element) {
    const view = getDocumentView(element.ownerDocument);
    const style = view.getComputedStyle(element);
    const rect = element.getBoundingClientRect();
    return {
      rect,
      visible: rect.width > 0 && rect.height > 0,
      cursor: style.cursor,
      inline: style.display === 'inline',
      pointerEvents: style.pointerEvents
    };
  }

  function receivesPointerEvents(element) {
    const box = computeBox(element);
    if (!box.visible) {
      return false;
    }
    return box.pointerEvents !== 'none';
  }

  function getInputRole(input) {
    const type = (input.type || 'text').toLowerCase();
    const roles = {
      button: 'button',
      checkbox: 'checkbox',
      radio: 'radio',
      range: 'slider',
      search: 'searchbox',
      text: 'textbox',
      email: 'textbox',
      tel: 'textbox',
      url: 'textbox',
      number: 'spinbutton'
    };
    return roles[type] || 'textbox';
  }

  function getAriaRole(element) {
    const explicitRole = element.getAttribute('role');
    if (explicitRole) {
      const roles = explicitRole.split(' ').map((role) => role.trim());
      if (roles[0]) {
        return roles[0];
      }
    }

    const tagName = element.tagName;
    const implicitRoles = {
      BUTTON: 'button',
      A: element.hasAttribute('href') ? 'link' : null,
      INPUT: getInputRole(element),
      TEXTAREA: 'textbox',
      SELECT: element.hasAttribute('multiple') || element.size > 1 ? 'listbox' : 'combobox',
      H1: 'heading',
      H2: 'heading',
      H3: 'heading',
      H4: 'heading',
      H5: 'heading',
      H6: 'heading',
      IMG: element.getAttribute('alt') === '' ? 'presentation' : 'img',
      NAV: 'navigation',
      MAIN: 'main',
      ARTICLE: 'article',
      SECTION: element.hasAttribute('aria-label') || element.hasAttribute('aria-labelledby') ? 'region' : null,
      HEADER: 'banner',
      FOOTER: 'contentinfo',
      ASIDE: 'complementary',
      FORM: 'form',
      TABLE: 'table',
      UL: 'list',
      OL: 'list',
      LI: 'listitem',
      P: 'paragraph',
      DIALOG: 'dialog',
      IFRAME: 'iframe'
    };

    return implicitRoles[tagName] || 'generic';
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

  function isActionableRole(role) {
    return [
      'button',
      'link',
      'textbox',
      'searchbox',
      'checkbox',
      'radio',
      'combobox',
      'listbox',
      'option',
      'menuitem',
      'menuitemcheckbox',
      'menuitemradio',
      'tab',
      'slider',
      'spinbutton',
      'switch',
      'dialog',
      'alertdialog'
    ].includes(role);
  }

  function isActionableElement(element) {
    const role = getAriaRole(element);
    const box = computeBox(element);
    return box.visible && (isActionableRole(role) || box.cursor === 'pointer');
  }

  function searchActionableIndex(targetIndex) {
    let currentIndex = 0;

    function visit(node, frameDepth) {
      if (!node) {
        return null;
      }

      if (node.nodeType !== 1) {
        return null;
      }

      const element = node;
      const visible = !isElementHiddenForAria(element) || isElementVisible(element);
      if (!visible) {
        return null;
      }

      if (isActionableElement(element)) {
        if (currentIndex === targetIndex) {
          return {
            element,
            frame_depth: frameDepth
          };
        }
        currentIndex += 1;
      }

      if (element.nodeName === 'SLOT') {
        const assignedNodes = element.assignedNodes();
        for (const child of assignedNodes) {
          const match = visit(child, frameDepth);
          if (match) {
            return match;
          }
        }
      } else {
        for (let child = element.firstChild; child; child = child.nextSibling) {
          if (!child.assignedSlot) {
            const match = visit(child, frameDepth);
            if (match) {
              return match;
            }
          }
        }

        if (element.shadowRoot) {
          for (let child = element.shadowRoot.firstChild; child; child = child.nextSibling) {
            const match = visit(child, frameDepth);
            if (match) {
              return match;
            }
          }
        }

        if (element.tagName === 'IFRAME') {
          try {
            const frameDoc = element.contentDocument;
            const frameWindow = element.contentWindow;
            if (frameDoc && frameWindow) {
              const frameRoot = frameDoc.body || frameDoc.documentElement;
              const match = visit(frameRoot, frameDepth + 1);
              if (match) {
                return match;
              }
            }
          } catch (error) {
            // Cross-origin frame; actionable lookup stops at the iframe boundary.
          }
        }
      }

      return null;
    }

    const root = document.body || document.documentElement;
    return visit(root, 0);
  }

  function findActionableIndexForElement(targetElement) {
    let currentIndex = 0;

    function visit(node) {
      if (!node || node.nodeType !== 1) {
        return null;
      }

      const element = node;
      const visible = !isElementHiddenForAria(element) || isElementVisible(element);
      if (!visible) {
        return null;
      }

      if (isActionableElement(element)) {
        if (element === targetElement) {
          return currentIndex;
        }
        currentIndex += 1;
      }

      if (element.nodeName === 'SLOT') {
        const assignedNodes = element.assignedNodes();
        for (const child of assignedNodes) {
          const match = visit(child);
          if (match !== null) {
            return match;
          }
        }
      } else {
        for (let child = element.firstChild; child; child = child.nextSibling) {
          if (!child.assignedSlot) {
            const match = visit(child);
            if (match) {
              return match;
            }
          }
        }
        if (element.shadowRoot) {
          for (let child = element.shadowRoot.firstChild; child; child = child.nextSibling) {
            const match = visit(child);
            if (match !== null) {
              return match;
            }
          }
        }

        if (element.tagName === 'IFRAME') {
          try {
            const frameDoc = element.contentDocument;
            const frameWindow = element.contentWindow;
            if (frameDoc && frameWindow) {
              const frameRoot = frameDoc.body || frameDoc.documentElement;
              const match = visit(frameRoot);
              if (match !== null) {
                return match;
              }
            }
          } catch (error) {
            // Cross-origin frame; actionable lookup stops at the iframe boundary.
          }
        }
      }

      return null;
    }

    const root = document.body || document.documentElement;
    return visit(root);
  }

  function querySelectorAcrossScopes(selector) {
    const visitedDocs = new Set();
    const boundaries = [];

    function searchRoot(root, frameDepth) {
      if (!root || typeof root.querySelector !== 'function') {
        return null;
      }

      let directMatch = null;
      try {
        directMatch = root.querySelector(selector);
      } catch (error) {
        return null;
      }

      if (directMatch) {
        return {
          element: directMatch,
          frame_depth: frameDepth
        };
      }

      const elements = root.querySelectorAll ? root.querySelectorAll('*') : [];
      for (const element of elements) {
        if (element.shadowRoot) {
          const shadowMatch = searchRoot(element.shadowRoot, frameDepth);
          if (shadowMatch) {
            return shadowMatch;
          }
        }

        if (element.tagName === 'IFRAME') {
          try {
            const frameDoc = element.contentDocument;
            if (!frameDoc) {
              boundaries.push({
                kind: 'iframe',
                status: 'unavailable',
                available: false,
                url: null
              });
              continue;
            }

            if (visitedDocs.has(frameDoc)) {
              continue;
            }

            visitedDocs.add(frameDoc);
            const frameMatch = searchRoot(frameDoc, frameDepth + 1);
            if (frameMatch) {
              return frameMatch;
            }
          } catch (error) {
            boundaries.push({
              kind: 'iframe',
              status: 'cross_origin',
              available: false,
              url: null
            });
          }
        }
      }

      return null;
    }

    visitedDocs.add(document);
    return {
      match: searchRoot(document, 0),
      boundaries
    };
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
      boundary: buildBoundary(element),
      sections: buildSections(element)
    };
  }

  try {
    let match = null;
    let selectorSearch = null;
    if (config.selector) {
      selectorSearch = querySelectorAcrossScopes(config.selector);
      match = selectorSearch.match;
    }

    if (!match && typeof config.target_index === 'number') {
      match = searchActionableIndex(config.target_index);
    }

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
