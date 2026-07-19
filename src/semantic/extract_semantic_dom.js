// Hydrated DOM semantic capture for the tui-gated SemanticDocument path.
// Separate from extract_dom.js (ARIA actionability). Avoids layout/geometry
// queries (no getBoundingClientRect); uses cheap visibility checks so client
// filters (e.g. Holmes toggling Tailwind `.hidden` / display:none) drop from
// the markdown view.
JSON.stringify((function() {
    'use strict';

    var MAX_COMPONENTS = 10000;
    var MAX_DEPTH = 64;
    var MAX_STRING_CHARS = 4096;
    var MAX_SELECT_OPTIONS = 256;
    var MAX_TOTAL_TEXT_CHARS = 1000000;

    var componentCount = 0;
    var totalTextChars = 0;
    var truncated = false;

    function getDocumentView(doc) {
        return doc.defaultView || window;
    }

    function generateDocumentId(doc) {
        var view = getDocumentView(doc);
        if (view.crypto && typeof view.crypto.randomUUID === 'function') {
            return view.crypto.randomUUID();
        }
        return 'doc-' + Math.random().toString(36).slice(2, 12);
    }

    function ensureDocumentState(doc) {
        var view = getDocumentView(doc);
        if (!view.__browserUseDocumentState) {
            var state = {
                documentId: generateDocumentId(doc),
                revision: 1,
                frameTrackerListeners: []
            };
            var observer = new view.MutationObserver(function() {
                state.revision += 1;
            });
            observer.observe(doc, {
                subtree: true,
                childList: true,
                attributes: true,
                characterData: true
            });
            state.observer = observer;
            view.__browserUseDocumentState = state;
        }
        return view.__browserUseDocumentState;
    }

    function observeOpenShadowRoot(root) {
        if (!root) return;
        var state = ensureDocumentState(root.ownerDocument);
        if (!state.observer) return;
        if (!state.semanticShadowRoots) {
            state.semanticShadowRoots = new WeakSet();
        }
        if (state.semanticShadowRoots.has(root)) return;
        state.observer.observe(root, {
            subtree: true,
            childList: true,
            attributes: true,
            characterData: true
        });
        state.semanticShadowRoots.add(root);
    }

    function clipString(value) {
        if (value == null) {
            return null;
        }
        var text = String(value);
        if (text.length > MAX_STRING_CHARS) {
            truncated = true;
            text = text.slice(0, MAX_STRING_CHARS);
        }
        totalTextChars += text.length;
        if (totalTextChars > MAX_TOTAL_TEXT_CHARS) {
            truncated = true;
        }
        return text;
    }

    function visibleText(element) {
        if (!element) {
            return '';
        }
        var text = element.innerText || element.textContent || '';
        return text.replace(/\s+/g, ' ').trim();
    }

    function directText(element) {
        var parts = [];
        for (var child = element.firstChild; child; child = child.nextSibling) {
            if (child.nodeType === 3) {
                var value = String(child.nodeValue || '').replace(/\s+/g, ' ').trim();
                if (value) {
                    parts.push(value);
                }
            }
        }
        return parts.join(' ').trim();
    }

    function landmarkRole(tag, role) {
        var normalized = (role || '').toLowerCase();
        if (normalized === 'main' || normalized === 'complementary' || normalized === 'banner' ||
            normalized === 'navigation' || normalized === 'region' || normalized === 'contentinfo') {
            if (normalized === 'complementary') return 'aside';
            if (normalized === 'banner') return 'header';
            if (normalized === 'navigation') return 'nav';
            if (normalized === 'region') return 'section';
            if (normalized === 'contentinfo') return 'footer';
            return normalized;
        }
        switch (tag) {
            case 'MAIN': return 'main';
            case 'ASIDE': return 'aside';
            case 'HEADER': return 'header';
            case 'NAV': return 'nav';
            case 'SECTION': return 'section';
            case 'FOOTER': return 'footer';
            default: return null;
        }
    }

    function isLandmark(tag, role) {
        return landmarkRole(tag, role) != null;
    }

    function isHeading(tag) {
        return tag === 'H1' || tag === 'H2' || tag === 'H3' ||
            tag === 'H4' || tag === 'H5' || tag === 'H6';
    }

    function headingLevel(tag) {
        return Number(tag.charAt(1));
    }

    function isSemanticElement(element) {
        var tag = element.tagName;
        var role = element.getAttribute('role');
        if (isLandmark(tag, role)) return true;
        if (isHeading(tag)) return true;
        if (tag === 'P' || tag === 'BLOCKQUOTE' || tag === 'PRE' || tag === 'FIGCAPTION') return true;
        if (tag === 'OL' || tag === 'UL' || tag === 'LI') return true;
        // Named anchors and href links are both semantic (fragment targets).
        if (tag === 'A' && (element.hasAttribute('href') || element.getAttribute('name'))) return true;
        if (tag === 'IMG') return true;
        if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || tag === 'BUTTON') return true;
        if (tag === 'ARTICLE' || tag === 'FIGURE' || tag === 'FIELDSET' || tag === 'FORM' || tag === 'DETAILS') {
            return true;
        }
        // Id-bearing wrappers become groups so #id targets remain addressable.
        if (element.id) return true;
        if (role === 'group' || role === 'list' || role === 'listitem' || role === 'link' ||
            role === 'button' || role === 'img' || role === 'textbox' || role === 'combobox') {
            return true;
        }
        return false;
    }

    function isSkippable(element) {
        var tag = element.tagName;
        if (tag === 'SCRIPT' || tag === 'STYLE' || tag === 'NOSCRIPT' ||
            tag === 'TEMPLATE' || tag === 'SVG' || tag === 'HEAD' || tag === 'META' ||
            tag === 'LINK' || tag === 'BR' || tag === 'HR') {
            return true;
        }
        // Client-side filters (Holmes, Alpine, etc.) hide non-matches with
        // [hidden], aria-hidden, or display:none / visibility:hidden. Without
        // this, the TUI keeps showing the full unfiltered list after search.
        if (isEffectivelyHidden(element)) {
            return true;
        }
        return false;
    }

    // True when the element is not presented to the user (no geometry probes).
    function isEffectivelyHidden(element) {
        if (!element || element.nodeType !== 1) {
            return false;
        }
        // HTML hidden property / attribute
        if (element.hidden === true) {
            return true;
        }
        if (element.getAttribute('hidden') != null) {
            return true;
        }
        if (element.getAttribute('aria-hidden') === 'true') {
            return true;
        }
        // Inline styles (common for scripted hide without class)
        var inline = element.style;
        if (inline) {
            if (inline.display === 'none' || inline.visibility === 'hidden') {
                return true;
            }
        }
        // Computed style: Tailwind `.hidden` and similar utility classes.
        try {
            var view = getDocumentView(element.ownerDocument || document);
            if (view && typeof view.getComputedStyle === 'function') {
                var style = view.getComputedStyle(element);
                if (style) {
                    if (style.display === 'none' || style.visibility === 'hidden') {
                        return true;
                    }
                }
            }
        } catch (error) {
            // Cross-origin / detached — treat as not hidden.
        }
        return false;
    }

    function collectSelectOptions(select) {
        var options = [];
        var list = select.options || [];
        for (var i = 0; i < list.length && options.length < MAX_SELECT_OPTIONS; i += 1) {
            var option = list[i];
            options.push({
                value: clipString(option.value || '') || '',
                label: clipString(option.label || option.text || '') || null,
                selected: !!option.selected,
                disabled: !!option.disabled
            });
        }
        if (list.length > MAX_SELECT_OPTIONS) {
            truncated = true;
        }
        return options;
    }

    function controlLabel(element) {
        if (element.id) {
            try {
                var label = document.querySelector('label[for="' + CSS.escape(element.id) + '"]');
                if (label) {
                    return clipString(visibleText(label));
                }
            } catch (error) {
                // CSS.escape may be unavailable; fall through.
            }
        }
        var aria = element.getAttribute('aria-label');
        if (aria) {
            return clipString(aria);
        }
        var labelledBy = element.getAttribute('aria-labelledby');
        if (labelledBy) {
            var parts = [];
            var ids = labelledBy.split(/\s+/);
            for (var i = 0; i < ids.length; i += 1) {
                var ref = document.getElementById(ids[i]);
                if (ref) {
                    parts.push(visibleText(ref));
                }
            }
            if (parts.length) {
                return clipString(parts.join(' '));
            }
        }
        if (element.labels && element.labels.length) {
            return clipString(visibleText(element.labels[0]));
        }
        return null;
    }

    function normalizeTextChunk(value) {
        if (value == null) {
            return '';
        }
        return String(value).replace(/\s+/g, ' ').trim();
    }

    function makeTextFragment(text) {
        var clipped = clipString(normalizeTextChunk(text));
        if (!clipped) {
            return null;
        }
        if (componentCount >= MAX_COMPONENTS || totalTextChars > MAX_TOTAL_TEXT_CHARS) {
            truncated = true;
            return null;
        }
        componentCount += 1;
        return {
            kind: 'text',
            tag: null,
            id: null,
            text: clipped,
            children: []
        };
    }

    function isPlainTextFragment(node) {
        return !!(
            node &&
            node.kind === 'text' &&
            (!node.children || node.children.length === 0) &&
            !node.href &&
            !node.landmark &&
            node.heading_level == null
        );
    }

    // Collapse runs of plain text-only ordered content into one compact leaf string.
    // Mixed semantic content is left as an ordered child list without aggregate text.
    function summarizeOrderedContent(ordered) {
        if (!ordered.length) {
            return { mode: 'empty' };
        }

        var parts = [];
        for (var i = 0; i < ordered.length; i += 1) {
            if (!isPlainTextFragment(ordered[i])) {
                return { mode: 'mixed', children: ordered };
            }
            if (ordered[i].text) {
                parts.push(ordered[i].text);
            }
        }

        return {
            mode: 'leaf',
            text: parts.join(' ')
        };
    }

    // Document-order walk: text nodes become Text fragments; semantic elements
    // become components; generic wrappers are unwrapped while retaining direct text.
    function visitOrdered(element, depth) {
        var children = [];
        if (depth > MAX_DEPTH) {
            truncated = true;
            return children;
        }

        function pushNodes(nodes) {
            for (var i = 0; i < nodes.length; i += 1) {
                if (nodes[i]) {
                    children.push(nodes[i]);
                }
            }
        }

        function visitChildNode(child) {
            if (!child) {
                return;
            }
            if (child.nodeType === 3) {
                var fragment = makeTextFragment(child.nodeValue);
                if (fragment) {
                    children.push(fragment);
                }
                return;
            }
            if (child.nodeType !== 1) {
                return;
            }
            if (child.assignedSlot) {
                return;
            }
            if (isSkippable(child)) {
                return;
            }
            if (!isSemanticElement(child)) {
                pushNodes(visitOrdered(child, depth + 1));
                return;
            }
            pushNodes(visit(child, depth));
        }

        for (var child = element.firstChild; child; child = child.nextSibling) {
            visitChildNode(child);
        }

        if (element.shadowRoot) {
            observeOpenShadowRoot(element.shadowRoot);
            for (var shadowChild = element.shadowRoot.firstChild; shadowChild; shadowChild = shadowChild.nextSibling) {
                visitChildNode(shadowChild);
            }
        }

        if (element.tagName === 'SLOT') {
            var assigned = element.assignedNodes();
            for (var j = 0; j < assigned.length; j += 1) {
                visitChildNode(assigned[j]);
            }
        }

        return children;
    }

    function makeNode(kind, element, fields, children) {
        if (componentCount >= MAX_COMPONENTS || totalTextChars > MAX_TOTAL_TEXT_CHARS) {
            truncated = true;
            return null;
        }
        componentCount += 1;

        var selector = null;
        if (kind === 'link' || kind === 'input' || kind === 'textarea' ||
            kind === 'select' || kind === 'button') {
            selector = buildInteractionSelector(element);
            if (selector) {
                totalTextChars += selector.length;
                if (totalTextChars > MAX_TOTAL_TEXT_CHARS) {
                    truncated = true;
                    return null;
                }
            }
        }

        var node = {
            kind: kind,
            tag: element.tagName.toLowerCase(),
            id: element.id ? clipString(element.id) : null,
            selector: selector,
            children: children || []
        };

        if (fields) {
            for (var key in fields) {
                if (Object.prototype.hasOwnProperty.call(fields, key) && fields[key] != null) {
                    node[key] = fields[key];
                }
            }
        }

        return node;
    }

    // Mint an exact, capture-scoped locator without relying on text, classes,
    // or mutable form values. Each JSON segment resolves uniquely inside the
    // document or an open shadow root; the next segment enters that host's
    // shadow root. Closed roots remain intentionally inaccessible.
    function buildInteractionSelector(element) {
        if (!element) return null;

        var segments = [];
        var target = element;
        while (target) {
            var root = target.getRootNode();
            if (!root || !root.querySelectorAll) return null;
            var selector = selectorWithinRoot(target, root);
            if (!selector) return null;
            segments.push(selector);
            if (root === document) break;
            if (!root.host) return null;
            target = root.host;
        }
        segments.reverse();
        var encoded = JSON.stringify(segments);
        return encoded.length <= MAX_STRING_CHARS ? encoded : null;
    }

    function selectorWithinRoot(element, root) {
        var path = [];
        var current = element;
        while (current && current.nodeType === 1) {
            var parent = current.parentElement;
            var tag = current.tagName.toLowerCase();
            if (parent) {
                var index = Array.prototype.indexOf.call(parent.children, current) + 1;
                if (index <= 0) return null;
                tag += ':nth-child(' + index + ')';
            } else if (current.parentNode === root && root.children) {
                var rootIndex = Array.prototype.indexOf.call(root.children, current) + 1;
                if (rootIndex <= 0) return null;
                tag += ':nth-child(' + rootIndex + ')';
            }
            path.unshift(tag);
            if (!parent) break;
            current = parent;
        }

        var selector = path.join(' > ');
        if (!selector || selector.length > MAX_STRING_CHARS) return null;
        try {
            var matches = root.querySelectorAll(selector);
            return matches.length === 1 && matches[0] === element ? selector : null;
        } catch (_) {
            return null;
        }
    }

    // Textual containers: leaf when only text; ordered children without aggregate
    // text when nested semantic descendants are present.
    function makeTextualContainer(kind, element, depth, extraFields) {
        var ordered = visitOrdered(element, depth + 1);
        var summary = summarizeOrderedContent(ordered);
        var fields = extraFields || {};

        if (summary.mode === 'empty') {
            return null;
        }

        if (summary.mode === 'leaf') {
            // Drop provisional text-fragment nodes; only the compact container remains.
            for (var i = 0; i < ordered.length; i += 1) {
                componentCount -= 1;
                if (ordered[i].text) {
                    totalTextChars = Math.max(0, totalTextChars - ordered[i].text.length);
                }
            }
            // summary.text is already clipped via fragment construction.
            fields.text = summary.text;
            if (kind === 'heading' || kind === 'list_item') {
                fields.label = summary.text;
            }
            // Re-apply string budget for the retained container text once.
            totalTextChars += summary.text.length;
            return makeNode(kind, element, fields, []);
        }

        // Mixed: ordered children are authoritative; never keep aggregate innerText.
        return makeNode(kind, element, fields, summary.children);
    }

    function visit(element, depth) {
        if (!element || element.nodeType !== 1) {
            return [];
        }
        if (isSkippable(element)) {
            return [];
        }
        if (componentCount >= MAX_COMPONENTS || totalTextChars > MAX_TOTAL_TEXT_CHARS) {
            truncated = true;
            return [];
        }
        if (depth > MAX_DEPTH) {
            truncated = true;
            return [];
        }

        var tag = element.tagName;
        var role = element.getAttribute('role');

        // Generic layout wrappers: unwrap, keeping ordered text + semantic children.
        if (!isSemanticElement(element)) {
            return visitOrdered(element, depth + 1);
        }

        var children;
        var node = null;

        if (isLandmark(tag, role)) {
            children = visitOrdered(element, depth + 1);
            node = makeNode('landmark', element, {
                landmark: landmarkRole(tag, role),
                label: clipString(element.getAttribute('aria-label') || '')
            }, children);
        } else if (isHeading(tag)) {
            node = makeTextualContainer('heading', element, depth, {
                heading_level: headingLevel(tag)
            });
        } else if (tag === 'P' || tag === 'BLOCKQUOTE' || tag === 'PRE' || tag === 'FIGCAPTION') {
            node = makeTextualContainer('text', element, depth, {});
        } else if (tag === 'OL' || tag === 'UL' || role === 'list') {
            children = visitOrdered(element, depth + 1);
            node = makeNode('list', element, {
                ordered: tag === 'OL'
            }, children);
        } else if (tag === 'LI' || role === 'listitem') {
            node = makeTextualContainer('list_item', element, depth, {});
        } else if (tag === 'A' || role === 'link') {
            // Terminal leaf: aggregate label only; never fragment descendants.
            // Named anchors (name= without href) are still links for fragment targets.
            var linkText = clipString(visibleText(element));
            var hrefAttr = element.getAttribute('href');
            var nameAttr = element.getAttribute('name');
            node = makeNode('link', element, {
                href: hrefAttr != null ? clipString(hrefAttr) : '',
                name: nameAttr ? clipString(nameAttr) : null,
                text: linkText,
                label: linkText || (nameAttr ? clipString(nameAttr) : null)
            }, []);
        } else if (tag === 'IMG' || role === 'img') {
            node = makeNode('image', element, {
                src: clipString(element.getAttribute('src') || ''),
                alt: clipString(element.getAttribute('alt') || ''),
                label: clipString(element.getAttribute('alt') || element.getAttribute('aria-label') || '')
            }, []);
        } else if (tag === 'INPUT') {
            var inputType = (element.getAttribute('type') || 'text').toLowerCase();
            if (inputType === 'hidden') {
                return [];
            }
            node = makeNode('input', element, {
                input_type: clipString(inputType),
                name: clipString(element.getAttribute('name') || ''),
                value: clipString(element.value != null ? element.value : element.getAttribute('value') || ''),
                placeholder: clipString(element.getAttribute('placeholder') || ''),
                checked: element.checked ? true : null,
                disabled: element.disabled ? true : null,
                required: element.required ? true : null,
                readonly: element.readOnly ? true : null,
                label: controlLabel(element)
            }, []);
        } else if (tag === 'TEXTAREA' || role === 'textbox') {
            node = makeNode('textarea', element, {
                name: clipString(element.getAttribute('name') || ''),
                value: clipString(element.value != null ? element.value : ''),
                placeholder: clipString(element.getAttribute('placeholder') || ''),
                disabled: element.disabled ? true : null,
                required: element.required ? true : null,
                readonly: element.readOnly ? true : null,
                label: controlLabel(element)
            }, []);
        } else if (tag === 'SELECT' || role === 'combobox' || role === 'listbox') {
            node = makeNode('select', element, {
                name: clipString(element.getAttribute('name') || ''),
                value: clipString(element.value != null ? element.value : ''),
                multiple: element.multiple ? true : null,
                disabled: element.disabled ? true : null,
                required: element.required ? true : null,
                options: collectSelectOptions(element),
                label: controlLabel(element)
            }, []);
        } else if (tag === 'BUTTON' || role === 'button') {
            // Terminal leaf: aggregate label only.
            var buttonText = clipString(visibleText(element) || element.getAttribute('value') || '');
            node = makeNode('button', element, {
                button_type: clipString(element.getAttribute('type') || 'submit'),
                name: clipString(element.getAttribute('name') || ''),
                value: clipString(element.getAttribute('value') || ''),
                disabled: element.disabled ? true : null,
                text: buttonText,
                label: buttonText || controlLabel(element)
            }, []);
        } else {
            // Generic semantic group (article, figure, form, fieldset, details,
            // role=group, or id-bearing wrappers kept for fragment targets).
            children = visitOrdered(element, depth + 1);
            if (!children.length) {
                var groupText = clipString(visibleText(element));
                // Keep empty id-bearing nodes so #id can still resolve.
                if (!groupText && !element.id) {
                    return [];
                }
                node = makeNode('group', element, {
                    text: groupText,
                    label: clipString(element.getAttribute('aria-label') || element.id || '')
                }, []);
            } else {
                node = makeNode('group', element, {
                    label: clipString(element.getAttribute('aria-label') || '')
                }, children);
            }
        }

        if (!node) {
            return [];
        }
        return [node];
    }

    // Count ids across the full live DOM, including nodes omitted from the
    // semantic projection. An emitted node may only receive an author identity
    // when its id is unique at the browser interaction boundary as well.
    function markUniqueIds(nodes, root) {
        var counts = Object.create(null);

        if (root && root.querySelectorAll) {
            var allWithId = root.querySelectorAll('[id]');
            for (var i = 0; i < allWithId.length; i += 1) {
                var id = allWithId[i].id;
                if (id) counts[id] = (counts[id] || 0) + 1;
            }
        }

        function walk(list) {
            for (var i = 0; i < list.length; i += 1) {
                var node = list[i];
                if (node.id) {
                    counts[node.id] = (counts[node.id] || 0) + 1;
                }
                if (node.children && node.children.length) {
                    walk(node.children);
                }
            }
        }

        function apply(list) {
            for (var i = 0; i < list.length; i += 1) {
                var node = list[i];
                node.unique_id = !!(node.id && counts[node.id] === 1);
                if (node.children && node.children.length) {
                    apply(node.children);
                }
            }
        }

        // The query above is authoritative for a real DOM capture. Keep the
        // tree walk for synthetic/fixture roots that do not implement it.
        if (!root || !root.querySelectorAll) walk(nodes);
        apply(nodes);
    }

    try {
        var documentState = ensureDocumentState(document);
        var root = document.body || document.documentElement;
        var nodes = root ? visit(root, 0) : [];
        markUniqueIds(nodes, root);

        // Semantic capture deliberately models the top-level document only:
        // iframe content is neither walked nor exposed as semantic components.
        // Keep its revision in the same canonical main-frame namespace as the
        // browser metadata probe (`main:<revision>`), so publication can prove
        // that the represented DOM did not change after this capture. Frame
        // suffixes are intentionally excluded because they describe content
        // outside this semantic document.
        return {
            document: {
                document_id: documentState.documentId,
                revision: 'main:' + String(documentState.revision),
                url: document.location.href,
                title: document.title || '',
                ready_state: document.readyState,
                frames: []
            },
            nodes: nodes,
            truncated: truncated,
            error: null
        };
    } catch (error) {
        return {
            document: {
                document_id: '',
                revision: '',
                url: '',
                title: '',
                ready_state: '',
                frames: []
            },
            nodes: [],
            truncated: false,
            error: String(error && error.message ? error.message : error)
        };
    }
})());
