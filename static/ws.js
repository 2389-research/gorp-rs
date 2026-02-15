// ABOUTME: WebSocket client for real-time admin panel updates
// ABOUTME: Subscribe/unsubscribe model with auto-reconnect and handler registration

class GorpSocket {
    constructor() {
        this.ws = null;
        this.subscriptions = new Set();
        this.handlers = {};
        this.reconnectDelay = 2000;
        this.connect();
    }

    connect() {
        const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
        this.ws = new WebSocket(`${protocol}//${location.host}/admin/ws`);

        this.ws.onopen = () => {
            // Re-subscribe to channels on reconnect
            if (this.subscriptions.size > 0) {
                this.send({
                    type: 'subscribe',
                    channels: Array.from(this.subscriptions)
                });
            }
        };

        this.ws.onmessage = (e) => {
            try {
                this.dispatch(JSON.parse(e.data));
            } catch (err) {
                console.warn('Failed to parse WebSocket message:', err);
            }
        };

        this.ws.onclose = () => {
            setTimeout(() => this.connect(), this.reconnectDelay);
        };

        this.ws.onerror = () => {
            // onclose will fire after onerror, triggering reconnect
        };
    }

    subscribe(channels) {
        channels.forEach(ch => this.subscriptions.add(ch));
        if (this.ws && this.ws.readyState === WebSocket.OPEN) {
            this.send({ type: 'subscribe', channels: channels });
        }
    }

    unsubscribe(channels) {
        channels.forEach(ch => this.subscriptions.delete(ch));
        if (this.ws && this.ws.readyState === WebSocket.OPEN) {
            this.send({ type: 'unsubscribe', channels: channels });
        }
    }

    on(type, handler) {
        if (!this.handlers[type]) {
            this.handlers[type] = [];
        }
        this.handlers[type].push(handler);
    }

    off(type, handler) {
        if (this.handlers[type]) {
            this.handlers[type] = this.handlers[type].filter(h => h !== handler);
        }
    }

    send(msg) {
        if (this.ws && this.ws.readyState === WebSocket.OPEN) {
            this.ws.send(JSON.stringify(msg));
        }
    }

    dispatch(msg) {
        const type = msg.type;
        if (this.handlers[type]) {
            this.handlers[type].forEach(handler => handler(msg));
        }
        // Wildcard handlers
        if (this.handlers['*']) {
            this.handlers['*'].forEach(handler => handler(msg));
        }
    }

    chatSend(workspace, body) {
        this.send({ type: 'chat.send', workspace: workspace, body: body });
    }

    chatCancel(workspace) {
        this.send({ type: 'chat.cancel', workspace: workspace });
    }

    selectWorkspace(workspace) {
        this.send({ type: 'chat.select_workspace', workspace: workspace });
    }
}

window.gorp = new GorpSocket();
