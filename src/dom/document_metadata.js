JSON.stringify((function() {
    'use strict';

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
                // Ignore invalidation listener failures; metadata reads will rebuild on demand.
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

    function collectIframeElements(doc) {
        const root = doc.body || doc.documentElement;
        if (!root) {
            return [];
        }

        const results = [];

        function visit(node) {
            if (!node || node.nodeType !== 1) {
                return;
            }

            const element = node;
            if (element.tagName === 'IFRAME') {
                results.push(element);
            }

            if (element.tagName === 'SLOT') {
                for (const child of element.assignedNodes()) {
                    visit(child);
                }
                return;
            }

            for (let child = element.firstChild; child; child = child.nextSibling) {
                if (!child.assignedSlot) {
                    visit(child);
                }
            }

            if (element.shadowRoot) {
                for (let child = element.shadowRoot.firstChild; child; child = child.nextSibling) {
                    visit(child);
                }
            }
        }

        visit(root);
        return results;
    }

    function addFrameTrackerListener(doc, listener) {
        const state = ensureDocumentState(doc);
        state.frameTrackerListeners.push(listener);
        return function() {
            const index = state.frameTrackerListeners.indexOf(listener);
            if (index >= 0) {
                state.frameTrackerListeners.splice(index, 1);
            }
        };
    }

    function cleanupFrameTracker(tracker) {
        for (const cleanup of tracker.cleanupFns || []) {
            try {
                cleanup();
            } catch (error) {
                // Best-effort cleanup only.
            }
        }
        tracker.cleanupFns = [];
        tracker.entries = [];
    }

    function ensureFrameTracker(doc) {
        const view = getDocumentView(doc);
        const documentState = ensureDocumentState(doc);
        let tracker = view.__browserUseFrameTracker;

        if (!tracker || tracker.rootDocumentId !== documentState.documentId) {
            if (tracker) {
                cleanupFrameTracker(tracker);
            }
            tracker = {
                rootDocumentId: documentState.documentId,
                dirty: true,
                cleanupFns: [],
                entries: []
            };
            view.__browserUseFrameTracker = tracker;
        }

        return tracker;
    }

    function discoverTrackedFrames(doc, tracker, invalidate) {
        const entries = [];
        const iframeElements = collectIframeElements(doc);

        for (const iframeElement of iframeElements) {
            const entry = {
                iframe: iframeElement,
                document: null,
                children: []
            };
            entries.push(entry);
            tracker.cleanupFns.push(addIframeLoadInvalidator(iframeElement, invalidate));

            try {
                const frameDoc = iframeElement.contentDocument;
                const frameWindow = iframeElement.contentWindow;

                if (!frameDoc || !frameWindow) {
                    continue;
                }

                entry.document = frameDoc;
                tracker.cleanupFns.push(addFrameTrackerListener(frameDoc, invalidate));
                entry.children = discoverTrackedFrames(frameDoc, tracker, invalidate);
            } catch (error) {
                // Cross-origin frame; leave the document unset and sample the status lazily.
            }
        }

        return entries;
    }

    function addIframeLoadInvalidator(iframeElement, invalidate) {
        iframeElement.addEventListener('load', invalidate);
        return function() {
            iframeElement.removeEventListener('load', invalidate);
        };
    }

    function rebuildFrameTracker(doc, tracker) {
        cleanupFrameTracker(tracker);
        const invalidate = function() {
            tracker.dirty = true;
        };
        tracker.cleanupFns.push(addFrameTrackerListener(doc, invalidate));
        tracker.entries = discoverTrackedFrames(doc, tracker, invalidate);
        tracker.dirty = false;
    }

    function sampleTrackedFrameReports(doc) {
        const tracker = ensureFrameTracker(doc);
        if (tracker.dirty) {
            rebuildFrameTracker(doc, tracker);
        }

        let frameReports = [];
        if (!sampleFrameEntries(tracker.entries, frameReports, tracker)) {
            rebuildFrameTracker(doc, tracker);
            frameReports = [];
            sampleFrameEntries(tracker.entries, frameReports, tracker);
        }

        return frameReports;
    }

    function sampleFrameEntries(entries, reports, tracker) {
        for (const entry of entries) {
            if (!sampleFrameEntry(entry, reports, tracker)) {
                return false;
            }
        }

        return true;
    }

    function sampleFrameEntry(entry, reports, tracker) {
        if (!entry.iframe || !entry.iframe.isConnected) {
            tracker.dirty = true;
            return false;
        }

        const report = {
            status: 'unavailable'
        };

        try {
            const frameDoc = entry.iframe.contentDocument;
            const frameWindow = entry.iframe.contentWindow;

            if (!frameDoc || !frameWindow) {
                reports.push(report);
                return true;
            }

            if (entry.document !== frameDoc) {
                tracker.dirty = true;
                return false;
            }

            report.url = frameWindow.location.href;
            const frameState = ensureDocumentState(frameDoc);
            report.document_id = frameState.documentId;
            report.revision = String(frameState.revision);

            const frameRootElement = frameDoc.body || frameDoc.documentElement;
            if (!frameRootElement) {
                report.status = 'expanded_empty';
                reports.push(report);
                return true;
            }

            report.status = 'expanded';
            reports.push(report);

            if (!sampleFrameEntries(entry.children, reports, tracker)) {
                return false;
            }

            return true;
        } catch (error) {
            if (entry.document) {
                tracker.dirty = true;
                return false;
            }

            report.status = 'cross_origin';
            reports.push(report);
            return true;
        }
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

    const documentState = ensureDocumentState(document);
    const frameReports = sampleTrackedFrameReports(document);

    return {
        document_id: documentState.documentId,
        revision: buildRevisionToken(documentState, frameReports),
        url: document.location.href,
        title: document.title || '',
        ready_state: document.readyState,
        frames: frameReports
    };
})());
