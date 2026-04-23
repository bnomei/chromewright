// ARIA Snapshot DOM Extraction
// Based on Playwright's ariaSnapshot.ts - generates ARIA-tree structure for AI consumption
JSON.stringify((function() {
    'use strict';

    let currentIndex = 0;

    function getDocumentView(doc) {
        return doc.defaultView || window;
    }

    function generateDocumentId(doc) {
        const view = getDocumentView(doc);
        if (view.crypto && typeof view.crypto.randomUUID === 'function') {
            return view.crypto.randomUUID();
        }
        return 'doc-' + Math.random().toString(36).slice(2, 12);
    }

    function ensureDocumentState(doc) {
        const view = getDocumentView(doc);
        if (!view.__browserUseDocumentState) {
            const state = {
                documentId: generateDocumentId(doc),
                revision: 1,
                frameTrackerListeners: []
            };
            const observer = new view.MutationObserver(function(mutations) {
                state.revision += 1;
                notifyFrameTrackerListeners(state, mutations);
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
        const state = view.__browserUseDocumentState;
        if (!Array.isArray(state.frameTrackerListeners)) {
            state.frameTrackerListeners = [];
        }
        return state;
    }

    function notifyFrameTrackerListeners(documentState, mutations) {
        if (!hasFrameStructureMutation(mutations)) {
            return;
        }

        const listeners = documentState.frameTrackerListeners || [];
        for (const listener of listeners.slice()) {
            try {
                listener();
            } catch (error) {
                // Ignore tracker invalidation failures during extraction.
            }
        }
    }

    function hasFrameStructureMutation(mutations) {
        for (const mutation of mutations || []) {
            if (mutation.type === 'attributes') {
                if (containsIframeNode(mutation.target)) {
                    return true;
                }
                continue;
            }

            if (mutation.type !== 'childList') {
                continue;
            }

            for (const node of mutation.addedNodes || []) {
                if (containsIframeNode(node)) {
                    return true;
                }
            }

            for (const node of mutation.removedNodes || []) {
                if (containsIframeNode(node)) {
                    return true;
                }
            }
        }

        return false;
    }

    function containsIframeNode(node) {
        if (!node || node.nodeType !== 1) {
            return false;
        }

        const element = node;
        if (element.tagName === 'IFRAME') {
            return true;
        }

        for (let child = element.firstChild; child; child = child.nextSibling) {
            if (!child.assignedSlot && containsIframeNode(child)) {
                return true;
            }
        }

        if (element.shadowRoot) {
            for (let child = element.shadowRoot.firstChild; child; child = child.nextSibling) {
                if (containsIframeNode(child)) {
                    return true;
                }
            }
        }

        if (element.tagName === 'SLOT') {
            for (const child of element.assignedNodes()) {
                if (containsIframeNode(child)) {
                    return true;
                }
            }
        }

        return false;
    }

    // Helper: normalize whitespace
    function normalizeWhiteSpace(text) {
        return text.replace(/\s+/g, ' ').trim();
    }

    // Helper: check if element is visible for ARIA
    function isElementHiddenForAria(element) {
        const tagName = element.tagName;
        if (['STYLE', 'SCRIPT', 'NOSCRIPT', 'TEMPLATE'].includes(tagName)) {
            return true;
        }

        const style = getDocumentView(element.ownerDocument).getComputedStyle(element);
        
        // Check display: contents
        if (style.display === 'contents' && element.nodeName !== 'SLOT') {
            let hasVisibleChild = false;
            for (let child of element.childNodes) {
                if (child.nodeType === 1 && !isElementHiddenForAria(child)) {
                    hasVisibleChild = true;
                    break;
                }
                if (child.nodeType === 3 && child.textContent && child.textContent.trim()) {
                    hasVisibleChild = true;
                    break;
                }
            }
            if (!hasVisibleChild) return true;
        }
        
        // Check visibility
        if (style.visibility !== 'visible') {
            return true;
        }
        
        // Check display: none and aria-hidden
        if (style.display === 'none') {
            return true;
        }
        
        if (element.getAttribute('aria-hidden') === 'true') {
            return true;
        }
        
        return false;
    }

    // Helper: check if element is visible (bounding box check)
    function isElementVisible(element) {
        const rect = element.getBoundingClientRect();
        return rect.width > 0 && rect.height > 0;
    }

    const PERSISTENT_EDGE_THRESHOLD = 96;
    const MAX_PERSISTENT_CHROME_HEIGHT_RATIO = 0.45;

    function nextContainingElement(element) {
        if (!element) {
            return null;
        }

        if (element.parentElement) {
            return element.parentElement;
        }

        const root = typeof element.getRootNode === 'function' ? element.getRootNode() : null;
        return root && root.host && root.host.nodeType === 1 ? root.host : null;
    }

    function detectPersistentChrome(element, view) {
        let current = element;

        while (current && current.nodeType === 1) {
            const style = view.getComputedStyle(current);
            const position = style ? style.position : '';
            if (position === 'fixed' || position === 'sticky') {
                const rect = current.getBoundingClientRect();
                const visible = rect.width > 0 && rect.height > 0;
                const inViewport =
                    rect.bottom > 0 &&
                    rect.right > 0 &&
                    rect.top < view.innerHeight &&
                    rect.left < view.innerWidth;
                const heightRatio = view.innerHeight > 0 ? rect.height / view.innerHeight : 1;
                const topPinned =
                    rect.top <= PERSISTENT_EDGE_THRESHOLD &&
                    rect.bottom > 0;
                const bottomPinned =
                    rect.bottom >= view.innerHeight - PERSISTENT_EDGE_THRESHOLD &&
                    rect.top < view.innerHeight;

                if (visible && inViewport && heightRatio <= MAX_PERSISTENT_CHROME_HEIGHT_RATIO) {
                    if (topPinned) {
                        return {
                            persistentChrome: true,
                            persistentPosition: position,
                            persistentEdge: 'top'
                        };
                    }

                    if (bottomPinned) {
                        return {
                            persistentChrome: true,
                            persistentPosition: position,
                            persistentEdge: 'bottom'
                        };
                    }
                }
            }

            current = nextContainingElement(current);
        }

        return {
            persistentChrome: false,
            persistentPosition: undefined,
            persistentEdge: undefined
        };
    }

    // Helper: compute element box information
    function computeBox(element) {
        const view = getDocumentView(element.ownerDocument);
        const style = getDocumentView(element.ownerDocument).getComputedStyle(element);
        const rect = element.getBoundingClientRect();
        const visible = rect.width > 0 && rect.height > 0;
        const inViewport =
            rect.bottom > 0 &&
            rect.right > 0 &&
            rect.top < view.innerHeight &&
            rect.left < view.innerWidth;
        const inline = style.display === 'inline';
        const cursor = style.cursor;
        const persistent = detectPersistentChrome(element, view);

        return {
            visible,
            inViewport,
            inline,
            cursor,
            rect,
            persistentChrome: persistent.persistentChrome,
            persistentPosition: persistent.persistentPosition,
            persistentEdge: persistent.persistentEdge
        };
    }

    // Helper: check if element receives pointer events
    function receivesPointerEvents(element) {
        const box = computeBox(element);
        if (!box.visible) return false;
        
        const style = getDocumentView(element.ownerDocument).getComputedStyle(element);
        return style.pointerEvents !== 'none';
    }

    // Helper: get ARIA role for element
    function getAriaRole(element) {
        // Check explicit role
        const explicitRole = element.getAttribute('role');
        if (explicitRole) {
            const roles = explicitRole.split(' ').map(r => r.trim());
            const validRole = roles[0]; // take first role
            if (validRole) return validRole;
        }
        
        // Implicit roles based on tag name
        const tagName = element.tagName;
        const implicitRoles = {
            'BUTTON': 'button',
            'A': element.hasAttribute('href') ? 'link' : null,
            'INPUT': getInputRole(element),
            'TEXTAREA': 'textbox',
            'SELECT': element.hasAttribute('multiple') || element.size > 1 ? 'listbox' : 'combobox',
            'H1': 'heading', 'H2': 'heading', 'H3': 'heading',
            'H4': 'heading', 'H5': 'heading', 'H6': 'heading',
            'IMG': element.getAttribute('alt') === '' ? 'presentation' : 'img',
            'NAV': 'navigation',
            'MAIN': 'main',
            'ARTICLE': 'article',
            'SECTION': element.hasAttribute('aria-label') || element.hasAttribute('aria-labelledby') ? 'region' : null,
            'HEADER': 'banner',
            'FOOTER': 'contentinfo',
            'ASIDE': 'complementary',
            'FORM': 'form',
            'TABLE': 'table',
            'UL': 'list', 'OL': 'list',
            'LI': 'listitem',
            'P': 'paragraph',
            'DIALOG': 'dialog',
            'IFRAME': 'iframe'
        };
        
        return implicitRoles[tagName] || 'generic';
    }

    function getInputRole(input) {
        const type = (input.type || 'text').toLowerCase();
        const roles = {
            'button': 'button',
            'checkbox': 'checkbox',
            'radio': 'radio',
            'range': 'slider',
            'search': 'searchbox',
            'text': 'textbox',
            'email': 'textbox',
            'tel': 'textbox',
            'url': 'textbox',
            'number': 'spinbutton'
        };
        return roles[type] || 'textbox';
    }

    // Helper: get accessible name for element
    function getElementAccessibleName(element) {
        const doc = element.ownerDocument;

        // Check aria-label
        const ariaLabel = element.getAttribute('aria-label');
        if (ariaLabel) return ariaLabel;
        
        // Check aria-labelledby
        const labelledBy = element.getAttribute('aria-labelledby');
        if (labelledBy) {
            const ids = labelledBy.split(/\s+/);
            const texts = ids.map(id => {
                const el = doc.getElementById(id);
                return el ? el.textContent : '';
            }).filter(t => t);
            if (texts.length) return texts.join(' ');
        }
        
        // Check associated label (for inputs)
        if (element.tagName === 'INPUT' || element.tagName === 'TEXTAREA' || element.tagName === 'SELECT') {
            const id = element.id;
            if (id) {
                const label = doc.querySelector('label[for="' + id + '"]');
                if (label) return label.textContent || '';
            }
            // Check if wrapped in label
            const parentLabel = element.closest('label');
            if (parentLabel) {
                return parentLabel.textContent || '';
            }
        }
        
        // Check alt for images
        if (element.tagName === 'IMG') {
            return element.getAttribute('alt') || '';
        }
        
        // Check title
        const title = element.getAttribute('title');
        if (title) return title;
        
        // Check placeholder for inputs
        if (element.tagName === 'INPUT' || element.tagName === 'TEXTAREA') {
            const placeholder = element.getAttribute('placeholder');
            if (placeholder) return placeholder;
        }
        
        // For links and buttons, use text content if no other name found
        if (element.tagName === 'A' || element.tagName === 'BUTTON') {
            const text = element.textContent || '';
            if (text.trim()) return text.trim();
        }
        
        return '';
    }

    // Helper: get ARIA checked state
    function getAriaChecked(element) {
        const checked = element.getAttribute('aria-checked');
        if (checked === 'true') return true;
        if (checked === 'false') return false;
        if (checked === 'mixed') return 'mixed';
        
        // Native checkbox/radio
        if (element.tagName === 'INPUT') {
            if (element.type === 'checkbox' || element.type === 'radio') {
                return element.checked;
            }
        }
        
        return undefined;
    }

    // Helper: get ARIA disabled state
    function getAriaDisabled(element) {
        const disabled = element.getAttribute('aria-disabled');
        if (disabled === 'true') return true;
        
        // Native disabled
        if (element.disabled !== undefined) {
            return element.disabled;
        }
        
        return undefined;
    }

    // Helper: get ARIA expanded state
    function getAriaExpanded(element) {
        const expanded = element.getAttribute('aria-expanded');
        if (expanded === 'true') return true;
        if (expanded === 'false') return false;
        return undefined;
    }

    // Helper: get ARIA level
    function getAriaLevel(element) {
        const level = element.getAttribute('aria-level');
        if (level) {
            const num = parseInt(level, 10);
            if (!isNaN(num)) return num;
        }
        
        // Heading level
        if (element.tagName.match(/^H[1-6]$/)) {
            return parseInt(element.tagName[1], 10);
        }
        
        return undefined;
    }

    // Helper: get ARIA pressed state
    function getAriaPressed(element) {
        const pressed = element.getAttribute('aria-pressed');
        if (pressed === 'true') return true;
        if (pressed === 'false') return false;
        if (pressed === 'mixed') return 'mixed';
        return undefined;
    }

    // Helper: get ARIA selected state
    function getAriaSelected(element) {
        const selected = element.getAttribute('aria-selected');
        if (selected === 'true') return true;
        if (selected === 'false') return false;
        return undefined;
    }

    // Helper: get CSS content (::before, ::after)
    function getCSSContent(element, pseudo) {
        try {
            const style = getDocumentView(element.ownerDocument).getComputedStyle(element, pseudo);
            const content = style.content;
            if (content && content !== 'none' && content !== 'normal') {
                // Simple extraction - remove quotes
                return content.replace(/^["']|["']$/g, '');
            }
        } catch (e) {
            // Ignore errors
        }
        return '';
    }

    function isActionableRole(role) {
        const actionableRoles = [
            'button', 'link', 'textbox', 'searchbox', 'checkbox', 'radio',
            'combobox', 'listbox', 'option', 'menuitem', 'menuitemcheckbox',
            'menuitemradio', 'tab', 'slider', 'spinbutton', 'switch',
            'dialog', 'alertdialog'
        ];
        return actionableRoles.includes(role);
    }

    // Compute ARIA index for element
    function computeAriaIndex(ariaNode, allowTargeting) {
        if (!allowTargeting || !ariaNode.box.visible) {
            return;
        }

        const hasPointerCursor = ariaNode.box.cursor === 'pointer';
        const isInteractiveRole = isActionableRole(ariaNode.role);
        
        if (!isInteractiveRole && !hasPointerCursor) {
            return;
        }
        
        ariaNode.index = currentIndex++;
    }

    // Convert element to AriaNode
    function toAriaNode(element, allowTargeting) {
        const active = element.ownerDocument.activeElement === element;
        
        // Handle iframe specially
        if (element.tagName === 'IFRAME') {
            const ariaNode = {
                role: 'iframe',
                name: '',
                tag: 'iframe',
                id: element.id || null,
                classes: typeof element.className === 'string'
                    ? element.className.trim().split(/\s+/).filter(Boolean)
                    : [],
                children: [],
                props: {},
                public_handle: false,
                element: element,
                box: computeBox(element),
                receivesPointerEvents: true,
                active: active
            };
            computeAriaIndex(ariaNode, allowTargeting);
            return ariaNode;
        }
        
        const role = getAriaRole(element);
        
        // Skip elements without role or with presentation/none
        if (!role || role === 'presentation' || role === 'none') {
            return null;
        }
        
        const name = normalizeWhiteSpace(getElementAccessibleName(element) || '');
        const box = computeBox(element);
        
        // Skip inline generic elements with just text
        if (role === 'generic' && box.inline && 
            element.childNodes.length === 1 && 
            element.childNodes[0].nodeType === 3) {
            return null;
        }
        
        const result = {
            role: role,
            name: name,
            tag: element.tagName.toLowerCase(),
            id: element.id || null,
            classes: typeof element.className === 'string'
                ? element.className.trim().split(/\s+/).filter(Boolean)
                : [],
            children: [],
            props: {},
            public_handle: false,
            element: element,
            box: box,
            receivesPointerEvents: receivesPointerEvents(element),
            active: active
        };
        
        computeAriaIndex(result, allowTargeting);
        
        // Add ARIA properties based on role
        const checkedRoles = ['checkbox', 'menuitemcheckbox', 'menuitemradio', 'radio', 'switch'];
        if (checkedRoles.includes(role)) {
            const checked = getAriaChecked(element);
            if (checked !== undefined) result.checked = checked;
        }
        
        const disabledRoles = ['button', 'input', 'select', 'textarea'];
        if (disabledRoles.includes(role) || role.includes('menuitem')) {
            const disabled = getAriaDisabled(element);
            if (disabled !== undefined) result.disabled = disabled;
        }
        
        const expandedRoles = ['button', 'combobox', 'gridcell', 'link', 'menuitem', 'row', 'tab', 'treeitem'];
        if (expandedRoles.includes(role)) {
            const expanded = getAriaExpanded(element);
            if (expanded !== undefined) result.expanded = expanded;
        }
        
        const levelRoles = ['heading', 'listitem', 'row', 'treeitem'];
        if (levelRoles.includes(role)) {
            const level = getAriaLevel(element);
            if (level !== undefined) result.level = level;
        }
        
        const pressedRoles = ['button'];
        if (pressedRoles.includes(role)) {
            const pressed = getAriaPressed(element);
            if (pressed !== undefined) result.pressed = pressed;
        }
        
        const selectedRoles = ['gridcell', 'option', 'row', 'tab', 'treeitem'];
        if (selectedRoles.includes(role)) {
            const selected = getAriaSelected(element);
            if (selected !== undefined) result.selected = selected;
        }
        
        // Special handling for input/textarea values
        if (element.tagName === 'INPUT' || element.tagName === 'TEXTAREA') {
            if (element.type !== 'checkbox' && element.type !== 'radio' && element.type !== 'file') {
                result.children = [element.value || ''];
            }
        }
        
        return result;
    }

    // Main visitor function
    function visit(ariaNode, node, parentElementVisible, visited, allowTargeting, frameReports) {
        if (visited.has(node)) return;
        visited.add(node);
        
        // Handle text nodes
        if (node.nodeType === 3) { // TEXT_NODE
            if (!parentElementVisible) return;
            
            const text = node.nodeValue;
            // Skip text inside textbox
            if (ariaNode.role !== 'textbox' && text) {
                ariaNode.children.push(text);
            }
            return;
        }
        
        // Only process element nodes
        if (node.nodeType !== 1) return; // ELEMENT_NODE
        
        const element = node;
        
        // Check visibility
        const isElementVisibleForAria = !isElementHiddenForAria(element);
        let visible = isElementVisibleForAria || isElementVisible(element);
        
        // Skip if not visible for ARIA
        if (!visible) return;
        
        // Handle aria-owns
        const ariaChildren = [];
        if (element.hasAttribute('aria-owns')) {
            const ids = element.getAttribute('aria-owns').split(/\s+/);
            for (const id of ids) {
                const ownedElement = element.ownerDocument.getElementById(id);
                if (ownedElement) ariaChildren.push(ownedElement);
            }
        }
        
        // Convert to aria node
        const childAriaNode = toAriaNode(element, allowTargeting);
        if (childAriaNode) {
            ariaNode.children.push(childAriaNode);
        }
        
        // Process element (add CSS content, children, etc.)
        processElement(
            childAriaNode || ariaNode,
            element,
            ariaChildren,
            visible,
            visited,
            allowTargeting,
            frameReports
        );
    }

    function processElement(ariaNode, element, ariaChildren, parentElementVisible, visited, allowTargeting, frameReports) {
        const style = getDocumentView(element.ownerDocument).getComputedStyle(element);
        const display = style ? style.display : 'inline';
        const treatAsBlock = (display !== 'inline' || element.nodeName === 'BR') ? ' ' : '';
        
        if (treatAsBlock) {
            ariaNode.children.push(treatAsBlock);
        }
        
        // Add ::before content
        const beforeContent = getCSSContent(element, '::before');
        if (beforeContent) {
            ariaNode.children.push(beforeContent);
        }
        
        // Process shadow DOM slots
        if (element.nodeName === 'SLOT') {
            const assignedNodes = element.assignedNodes();
            for (const child of assignedNodes) {
                visit(ariaNode, child, parentElementVisible, visited, allowTargeting, frameReports);
            }
        } else {
            // Process regular children
            for (let child = element.firstChild; child; child = child.nextSibling) {
                if (!child.assignedSlot) {
                    visit(ariaNode, child, parentElementVisible, visited, allowTargeting, frameReports);
                }
            }
            
            // Process shadow root
            if (element.shadowRoot) {
                for (let child = element.shadowRoot.firstChild; child; child = child.nextSibling) {
                    visit(ariaNode, child, parentElementVisible, visited, allowTargeting, frameReports);
                }
            }
        }
        
        // Process aria-owns children
        for (const child of ariaChildren) {
            visit(ariaNode, child, parentElementVisible, visited, allowTargeting, frameReports);
        }
        
        // Add ::after content
        const afterContent = getCSSContent(element, '::after');
        if (afterContent) {
            ariaNode.children.push(afterContent);
        }
        
        if (treatAsBlock) {
            ariaNode.children.push(treatAsBlock);
        }
        
        // Remove redundant children
        if (ariaNode.children.length === 1 && ariaNode.name === ariaNode.children[0]) {
            ariaNode.children = [];
        }
        
        // Add special props
        if (ariaNode.role === 'link' && element.hasAttribute('href')) {
            ariaNode.props.url = element.getAttribute('href');
        }
        
        if (ariaNode.role === 'textbox' && element.hasAttribute('placeholder')) {
            const placeholder = element.getAttribute('placeholder');
            if (placeholder !== ariaNode.name) {
                ariaNode.props.placeholder = placeholder;
            }
        }

        if (element.tagName === 'IFRAME') {
            expandIframeContent(ariaNode, element, frameReports);
        }
    }

    function expandIframeContent(ariaNode, iframeElement, frameReports) {
        const report = {
            index: ariaNode.index,
            status: 'unavailable'
        };

        try {
            const frameDoc = iframeElement.contentDocument;
            const frameWindow = iframeElement.contentWindow;

            if (!frameDoc || !frameWindow) {
                ariaNode.props.frame_status = 'unavailable';
                frameReports.push(report);
                return;
            }

            report.url = frameWindow.location.href;
            const frameState = ensureDocumentState(frameDoc);
            report.document_id = frameState.documentId;
            report.revision = String(frameState.revision);
            report.status = 'expanded';

            ariaNode.props.frame_status = 'expanded';
            ariaNode.props.frame_url = report.url;

            const frameRootElement = frameDoc.body || frameDoc.documentElement;
            if (!frameRootElement) {
                report.status = 'expanded_empty';
                ariaNode.props.frame_status = 'expanded_empty';
                frameReports.push(report);
                return;
            }

            const frameSnapshot = {
                role: 'fragment',
                name: '',
                children: [],
                props: {},
                element: frameRootElement,
                box: computeBox(frameRootElement),
                receivesPointerEvents: true
            };
            const frameVisited = new Set();
            visit(frameSnapshot, frameRootElement, true, frameVisited, true, frameReports);
            normalizeStringChildren(frameSnapshot);
            normalizeGenericRoles(frameSnapshot);
            ariaNode.children.push.apply(ariaNode.children, frameSnapshot.children);
        } catch (error) {
            report.status = 'cross_origin';
            ariaNode.props.frame_status = 'cross_origin';
        }

        frameReports.push(report);
    }

    function buildRevisionToken(documentState, frameReports) {
        const parts = ['main:' + documentState.revision];
        for (let i = 0; i < frameReports.length; i += 1) {
            const frame = frameReports[i];
            const frameKey = 'frame' + i;
            if (frame.document_id && frame.revision) {
                parts.push(frameKey + ':' + frame.document_id + ':' + frame.revision);
            } else {
                parts.push(frameKey + ':' + frame.status);
            }
        }
        return parts.join('|');
    }

    function buildDocumentMetadata(doc, documentState, frameReports) {
        return {
            document_id: documentState.documentId,
            revision: buildRevisionToken(documentState, frameReports),
            url: doc.location.href,
            title: doc.title || '',
            ready_state: doc.readyState,
            frames: frameReports
        };
    }

    // Normalize string children
    function normalizeStringChildren(ariaNode) {
        const normalizedChildren = [];
        let buffer = [];
        
        function flushBuffer() {
            if (buffer.length === 0) return;
            const text = normalizeWhiteSpace(buffer.join(''));
            if (text) {
                normalizedChildren.push(text);
            }
            buffer = [];
        }
        
        for (const child of ariaNode.children || []) {
            if (typeof child === 'string') {
                buffer.push(child);
            } else {
                flushBuffer();
                normalizeStringChildren(child);
                normalizedChildren.push(child);
            }
        }
        flushBuffer();
        
        ariaNode.children = normalizedChildren;
        
        // Remove if child equals name
        if (ariaNode.children.length === 1 && ariaNode.children[0] === ariaNode.name) {
            ariaNode.children = [];
        }
    }

    // Normalize generic roles (remove unnecessary generic wrappers)
    function normalizeGenericRoles(node) {
        function normalizeChildren(node) {
            const result = [];
            
            for (const child of node.children || []) {
                if (typeof child === 'string') {
                    result.push(child);
                    continue;
                }
                
                const normalized = normalizeChildren(child);
                result.push(...normalized);
            }
            
            // Remove generic that encloses single element
            const removeSelf = node.role === 'generic' && !node.name && 
                              result.length <= 1 && 
                              result.every(c => typeof c !== 'string' && c.index !== undefined);
            
            if (removeSelf) {
                return result;
            }
            
            node.children = result;
            return [node];
        }
        
        normalizeChildren(node);
    }

    // Serialize ariaNode to plain object (remove Element references)
    function serializeAriaNode(ariaNode) {
        const result = {
            role: ariaNode.role,
            name: ariaNode.name,
            children: [],
            props: ariaNode.props
        };

        if (ariaNode.tag) result.tag = ariaNode.tag;
        if (ariaNode.id) result.id = ariaNode.id;
        if (ariaNode.classes && ariaNode.classes.length > 0) result.classes = ariaNode.classes;
        if (ariaNode.public_handle) result.public_handle = true;
        
        // Include index if present
        if (ariaNode.index !== undefined) result.index = ariaNode.index;
        if (ariaNode.active) result.active = true;
        if (ariaNode.checked !== undefined) result.checked = ariaNode.checked;
        if (ariaNode.disabled !== undefined) result.disabled = ariaNode.disabled;
        if (ariaNode.expanded !== undefined) result.expanded = ariaNode.expanded;
        if (ariaNode.level !== undefined) result.level = ariaNode.level;
        if (ariaNode.pressed !== undefined) result.pressed = ariaNode.pressed;
        if (ariaNode.selected !== undefined) result.selected = ariaNode.selected;
        
        // Serialize box info
        result.box_info = {
            visible: ariaNode.box.visible,
            in_viewport: ariaNode.box.inViewport,
            cursor: ariaNode.box.cursor
        };
        if (ariaNode.box.persistentChrome) result.box_info.persistent_chrome = true;
        if (ariaNode.box.persistentPosition) {
            result.box_info.persistent_position = ariaNode.box.persistentPosition;
        }
        if (ariaNode.box.persistentEdge) {
            result.box_info.persistent_edge = ariaNode.box.persistentEdge;
        }
        
        // Serialize children
        for (const child of ariaNode.children) {
            if (typeof child === 'string') {
                result.children.push(child);
            } else {
                result.children.push(serializeAriaNode(child));
            }
        }
        
        return result;
    }

    // Collect selectors and iframe indices
    function collectSelectorsAndIframes(ariaNode, selectors, iframeIndices) {
        if (ariaNode.index !== undefined && ariaNode.element) {
            // Store CSS selector for element at its index position
            const selector = buildSelector(ariaNode.element);
            ariaNode.public_handle = Boolean(selector);
            // Ensure selectors array is large enough
            while (selectors.length <= ariaNode.index) {
                selectors.push('');
            }
            selectors[ariaNode.index] = selector;
            
            if (ariaNode.role === 'iframe') {
                iframeIndices.push(ariaNode.index);
            }
        }
        
        for (const child of ariaNode.children) {
            if (typeof child !== 'string') {
                collectSelectorsAndIframes(child, selectors, iframeIndices);
            }
        }
    }

    function escapeCssIdentifier(value) {
        const text = String(value || '');
        const css = getDocumentView(document).CSS;
        if (css && typeof css.escape === 'function') {
            return css.escape(text);
        }

        return text
            .replace(/[\0-\x1f\x7f]/g, function(char) {
                return '\\' + char.charCodeAt(0).toString(16) + ' ';
            })
            .replace(/^-?\d/, function(char) {
                return '\\' + char.charCodeAt(0).toString(16) + ' ';
            })
            .replace(/[^\w-]/g, function(char) {
                return '\\' + char;
            });
    }

    // Build CSS selector for element
    function buildSelector(element) {
        const doc = element.ownerDocument;
        if (element.id) {
            return '#' + escapeCssIdentifier(element.id);
        }
        
        const path = [];
        let current = element;
        
        while (current && current !== doc.body) {
            let selector = current.tagName.toLowerCase();
            
            if (current.className && typeof current.className === 'string') {
                const classes = current.className.trim().split(/\s+/);
                if (classes.length > 0 && classes[0]) {
                    selector += '.' + escapeCssIdentifier(classes[0]);
                }
            }
            
            // Add nth-child if needed for uniqueness
            const parent = current.parentElement;
            if (parent) {
                const siblings = Array.from(parent.children);
                const index = siblings.indexOf(current);
                if (siblings.filter(s => s.tagName === current.tagName).length > 1) {
                    selector += ':nth-child(' + (index + 1) + ')';
                }
            }
            
            path.unshift(selector);
            current = current.parentElement;
        }
        
        return path.join(' > ');
    }

    // Main execution
    try {
        const rootElement = document.body || document.documentElement;
        const visited = new Set();
        const frameReports = [];
        const documentState = ensureDocumentState(document);
        
        // Reset index counter
        currentIndex = 0;
        
        // Create root fragment node
        const snapshot = {
            role: 'fragment',
            name: '',
            children: [],
            props: {},
            element: rootElement,
            box: computeBox(rootElement),
            receivesPointerEvents: true
        };
        
        // Visit the DOM tree
        visit(snapshot, rootElement, true, visited, true, frameReports);
        
        // Normalize
        normalizeStringChildren(snapshot);
        normalizeGenericRoles(snapshot);
        
        // Collect selectors and iframe indices
        const selectors = [];
        const iframeIndices = [];
        collectSelectorsAndIframes(snapshot, selectors, iframeIndices);
        
        // Serialize and return
        const serialized = serializeAriaNode(snapshot);
        
        return {
            document: buildDocumentMetadata(document, documentState, frameReports),
            root: serialized,
            selectors: selectors,
            iframe_indices: iframeIndices
        };
        
    } catch (error) {
        return {
            document: {
                document_id: '',
                revision: '',
                url: document.location.href,
                title: document.title || '',
                ready_state: document.readyState,
                frames: []
            },
            error: error.toString(),
            root: {
                role: 'fragment',
                name: '',
                children: [],
                props: {},
                box_info: { visible: false, in_viewport: false }
            },
            selectors: [],
            iframe_indices: []
        };
    }
})())
