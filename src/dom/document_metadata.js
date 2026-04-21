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
                revision: 1
            };
            const observer = new view.MutationObserver(function() {
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

    function collectFrameReports(doc, reports) {
        const root = doc.body || doc.documentElement;
        if (!root) {
            return;
        }

        const iframeElements = root.querySelectorAll('iframe');
        for (const iframeElement of iframeElements) {
            const report = {
                status: 'unavailable'
            };

            try {
                const frameDoc = iframeElement.contentDocument;
                const frameWindow = iframeElement.contentWindow;

                if (!frameDoc || !frameWindow) {
                    reports.push(report);
                    continue;
                }

                report.url = frameWindow.location.href;
                const frameState = ensureDocumentState(frameDoc);
                report.document_id = frameState.documentId;
                report.revision = String(frameState.revision);

                const frameRootElement = frameDoc.body || frameDoc.documentElement;
                if (!frameRootElement) {
                    report.status = 'expanded_empty';
                    reports.push(report);
                    continue;
                }

                report.status = 'expanded';
                reports.push(report);
                collectFrameReports(frameDoc, reports);
            } catch (error) {
                report.status = 'cross_origin';
                reports.push(report);
            }
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
    const frameReports = [];
    collectFrameReports(document, frameReports);

    return {
        document_id: documentState.documentId,
        revision: buildRevisionToken(documentState, frameReports),
        url: document.location.href,
        title: document.title || '',
        ready_state: document.readyState,
        frames: frameReports
    };
})());
