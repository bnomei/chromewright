JSON.stringify(
  (function () {
    const config = __HOVER_CONFIG__;

    function getDocumentView(doc) {
      return doc.defaultView || window;
    }

    function isElementHiddenForAria(element) {
      const tagName = element.tagName;
      if (["STYLE", "SCRIPT", "NOSCRIPT", "TEMPLATE"].includes(tagName)) {
        return true;
      }

      const style = getDocumentView(element.ownerDocument).getComputedStyle(element);
      if (style.visibility !== "visible" || style.display === "none") {
        return true;
      }

      if (element.getAttribute("aria-hidden") === "true") {
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
      };
    }

    function getInputRole(input) {
      const type = (input.type || "text").toLowerCase();
      const roles = {
        button: "button",
        checkbox: "checkbox",
        radio: "radio",
        range: "slider",
        search: "searchbox",
        text: "textbox",
        email: "textbox",
        tel: "textbox",
        url: "textbox",
        number: "spinbutton",
      };
      return roles[type] || "textbox";
    }

    function getAriaRole(element) {
      const explicitRole = element.getAttribute("role");
      if (explicitRole) {
        const roles = explicitRole.split(" ").map((role) => role.trim());
        if (roles[0]) {
          return roles[0];
        }
      }

      const implicitRoles = {
        BUTTON: "button",
        A: element.hasAttribute("href") ? "link" : null,
        INPUT: getInputRole(element),
        TEXTAREA: "textbox",
        SELECT: element.hasAttribute("multiple") || element.size > 1 ? "listbox" : "combobox",
        DIALOG: "dialog",
      };

      return implicitRoles[element.tagName] || "generic";
    }

    function isActionableRole(role) {
      return [
        "button",
        "link",
        "textbox",
        "searchbox",
        "checkbox",
        "radio",
        "combobox",
        "listbox",
        "option",
        "menuitem",
        "menuitemcheckbox",
        "menuitemradio",
        "tab",
        "slider",
        "spinbutton",
        "switch",
        "dialog",
        "alertdialog",
      ].includes(role);
    }

    function isActionableElement(element) {
      const role = getAriaRole(element);
      const box = computeBox(element);
      return box.visible && (isActionableRole(role) || box.cursor === "pointer");
    }

    function searchActionableIndex(targetIndex) {
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
          if (currentIndex === targetIndex) {
            return element;
          }
          currentIndex += 1;
        }

        if (element.nodeName === "SLOT") {
          for (const child of element.assignedNodes()) {
            const match = visit(child);
            if (match) {
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
              if (match) {
                return match;
              }
            }
          }

          if (element.tagName === "IFRAME") {
            try {
              const frameDoc = element.contentDocument;
              const frameWindow = element.contentWindow;
              if (frameDoc && frameWindow) {
                const frameRoot = frameDoc.body || frameDoc.documentElement;
                const match = visit(frameRoot);
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
      return visit(root);
    }

    function querySelectorAcrossScopes(selector) {
      const visitedDocs = new Set();

      function searchRoot(root) {
        if (!root || typeof root.querySelector !== "function") {
          return null;
        }

        let directMatch = null;
        try {
          directMatch = root.querySelector(selector);
        } catch (error) {
          return null;
        }

        if (directMatch) {
          return directMatch;
        }

        const elements = root.querySelectorAll ? root.querySelectorAll("*") : [];
        for (const element of elements) {
          if (element.shadowRoot) {
            const shadowMatch = searchRoot(element.shadowRoot);
            if (shadowMatch) {
              return shadowMatch;
            }
          }

          if (element.tagName === "IFRAME") {
            try {
              const frameDoc = element.contentDocument;
              if (!frameDoc || visitedDocs.has(frameDoc)) {
                continue;
              }

              visitedDocs.add(frameDoc);
              const frameMatch = searchRoot(frameDoc);
              if (frameMatch) {
                return frameMatch;
              }
            } catch (error) {
              // Cross-origin frame; selector lookup stops at the iframe boundary.
            }
          }
        }

        return null;
      }

      visitedDocs.add(document);
      return searchRoot(document);
    }

    const selectorMatch = config.selector
      ? querySelectorAcrossScopes(config.selector)
      : null;
    const element =
      selectorMatch && selectorMatch.isConnected
        ? selectorMatch
        : typeof config.target_index === "number"
          ? searchActionableIndex(config.target_index)
          : null;
    if (!element || !element.isConnected) {
      return {
        success: false,
        code: "target_detached",
        error: "Element is no longer present",
      };
    }

    element.scrollIntoView({
      behavior: "auto",
      block: "center",
      inline: "center",
    });

    const view = getDocumentView(element.ownerDocument);
    const rect = element.getBoundingClientRect();
    const event = new view.MouseEvent("mouseover", {
      view,
      bubbles: true,
      cancelable: true,
      clientX: rect.left + rect.width / 2,
      clientY: rect.top + rect.height / 2,
    });

    element.dispatchEvent(event);

    return {
      success: true,
      tagName: element.tagName,
      id: element.id,
      className: typeof element.className === "string" ? element.className : "",
    };
  })()
);
