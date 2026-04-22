function getDocumentView(doc) {
  return doc.defaultView || window;
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

function visitActionableTree(node, frameDepth, visitor) {
  if (!node || node.nodeType !== 1) {
    return null;
  }

  const element = node;
  const visible = !isElementHiddenForAria(element) || isElementVisible(element);
  if (!visible) {
    return null;
  }

  if (isActionableElement(element)) {
    const match = visitor(element, frameDepth);
    if (match !== undefined && match !== null) {
      return match;
    }
  }

  if (element.nodeName === 'SLOT') {
    for (const child of element.assignedNodes()) {
      const match = visitActionableTree(child, frameDepth, visitor);
      if (match !== null) {
        return match;
      }
    }
  } else {
    for (let child = element.firstChild; child; child = child.nextSibling) {
      if (!child.assignedSlot) {
        const match = visitActionableTree(child, frameDepth, visitor);
        if (match !== null) {
          return match;
        }
      }
    }

    if (element.shadowRoot) {
      for (let child = element.shadowRoot.firstChild; child; child = child.nextSibling) {
        const match = visitActionableTree(child, frameDepth, visitor);
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
          const match = visitActionableTree(frameRoot, frameDepth + 1, visitor);
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

function searchActionableIndex(targetIndex) {
  let currentIndex = 0;
  const root = document.body || document.documentElement;
  return visitActionableTree(root, 0, (element, frameDepth) => {
    if (currentIndex === targetIndex) {
      return {
        element,
        frame_depth: frameDepth
      };
    }

    currentIndex += 1;
    return null;
  });
}

function findActionableIndexForElement(targetElement) {
  let currentIndex = 0;
  const root = document.body || document.documentElement;
  return visitActionableTree(root, 0, (element) => {
    if (element === targetElement) {
      return currentIndex;
    }

    currentIndex += 1;
    return null;
  });
}

function escapeCssIdentifier(value) {
  const text = String(value || '');
  if (typeof CSS !== 'undefined' && CSS && typeof CSS.escape === 'function') {
    return CSS.escape(text);
  }

  return text
    .replace(/[\0-\x1f\x7f]/g, (char) => '\\' + char.charCodeAt(0).toString(16) + ' ')
    .replace(/^-?\d/, (char) => '\\' + char.charCodeAt(0).toString(16) + ' ')
    .replace(/[^\w-]/g, (char) => '\\' + char);
}

function normalizeSimpleIdSelector(selector) {
  if (typeof selector !== 'string' || selector.length < 2 || selector[0] !== '#') {
    return null;
  }

  const rawId = selector.slice(1);
  if (!rawId || /\s/.test(rawId)) {
    return null;
  }

  const normalized = '#' + escapeCssIdentifier(rawId);
  return normalized === selector ? null : normalized;
}

function queryRootSelector(root, selector) {
  try {
    return root.querySelector(selector);
  } catch (error) {
    const normalized = normalizeSimpleIdSelector(selector);
    if (!normalized) {
      return null;
    }

    try {
      return root.querySelector(normalized);
    } catch (fallbackError) {
      return null;
    }
  }
}

function querySelectorAcrossScopes(selector, options) {
  const visitedDocs = new Set();
  const collectBoundaries = Boolean(options && options.collectBoundaries);
  const boundaries = [];

  function pushBoundary(status) {
    if (!collectBoundaries) {
      return;
    }

    boundaries.push({
      kind: 'iframe',
      status,
      available: false,
      url: null
    });
  }

  function searchRoot(root, frameDepth) {
    if (!root || typeof root.querySelector !== 'function') {
      return null;
    }

    const directMatch = queryRootSelector(root, selector);

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
            pushBoundary('unavailable');
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
          pushBoundary('cross_origin');
        }
      }
    }

    return null;
  }

  visitedDocs.add(document);
  const match = searchRoot(document, 0);
  if (collectBoundaries) {
    return {
      match,
      boundaries
    };
  }

  return match;
}

function resolveTargetMatch(config, options) {
  let selectorSearch = null;

  if (config.selector) {
    selectorSearch = querySelectorAcrossScopes(
      config.selector,
      options && options.collectBoundaries ? { collectBoundaries: true } : undefined
    );
    const selectorMatch =
      selectorSearch && selectorSearch.match !== undefined
        ? selectorSearch.match
        : selectorSearch;
    if (selectorMatch && selectorMatch.element && selectorMatch.element.isConnected) {
      return {
        match: selectorMatch,
        selector_search: selectorSearch
      };
    }
  }

  if (typeof config.target_index === 'number') {
    return {
      match: searchActionableIndex(config.target_index),
      selector_search: selectorSearch
    };
  }

  return {
    match: null,
    selector_search: selectorSearch
  };
}

function resolveTargetElement(config) {
  const resolved = resolveTargetMatch(config);
  const match = resolved.match;
  if (match && match.element && match.element.isConnected) {
    return match.element;
  }

  return null;
}

function selectorExistsAcrossScopes(selector) {
  const match = querySelectorAcrossScopes(selector);
  return Boolean(match && match.element);
}
