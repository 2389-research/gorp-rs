// ABOUTME: Chat page WebSocket client for streaming Claude conversations
// ABOUTME: Handles workspace selection, message sending, streaming chunks, and tool use display

(function() {
    var history = document.getElementById('chat-history');
    var form = document.getElementById('chat-form');
    var input = document.getElementById('chat-input');
    var sendBtn = document.getElementById('send-btn');
    var cancelBtn = document.getElementById('cancel-btn');
    var wsSelect = document.getElementById('workspace-select');
    var statusEl = document.getElementById('chat-status');
    var streamingEl = document.getElementById('streaming-indicator');
    var currentChunk = null;
    var streaming = false;

    // Subscribe to chat + status channels
    window.gorp.subscribe(['chat', 'status']);

    // Select workspace on change
    wsSelect.addEventListener('change', function() {
        var ws = wsSelect.value;
        if (ws) {
            window.gorp.selectWorkspace(ws);
            // Load history via HTMX-style fetch
            fetch('/admin/chat/' + encodeURIComponent(ws))
                .then(function(r) { return r.text(); })
                .then(function(html) { history.innerHTML = html; });
        }
    });

    // Send message
    form.addEventListener('submit', function(e) {
        e.preventDefault();
        var ws = wsSelect.value;
        var body = input.value.trim();
        if (!ws || !body) return;

        // Append user message
        appendMessage('user', body);
        input.value = '';

        // Send via WebSocket
        window.gorp.chatSend(ws, body);

        // Start streaming state
        setStreaming(true);
    });

    // Cancel streaming
    cancelBtn.addEventListener('click', function() {
        var ws = wsSelect.value;
        if (ws) window.gorp.chatCancel(ws);
    });

    // Handle streaming chunks
    window.gorp.on('chat.chunk', function(msg) {
        if (!currentChunk) {
            currentChunk = appendMessage('assistant', '');
        }
        var span = currentChunk.querySelector('.chat-content');
        span.textContent += msg.data.text;
        history.scrollTop = history.scrollHeight;
    });

    // Handle tool use
    window.gorp.on('chat.tool_use', function(msg) {
        var div = document.createElement('div');
        div.className = 'chat-msg text-purple-600 text-xs bg-purple-50 rounded px-2 py-1';
        div.innerHTML = '<span class="font-bold">Tool:</span> ' +
            escapeHtml(msg.data.tool) + '(' + escapeHtml(truncate(msg.data.input, 100)) + ')';
        history.appendChild(div);
        history.scrollTop = history.scrollHeight;
    });

    // Handle completion
    window.gorp.on('chat.complete', function() {
        currentChunk = null;
        setStreaming(false);
    });

    // Handle errors
    window.gorp.on('chat.error', function(msg) {
        currentChunk = null;
        setStreaming(false);
        statusEl.textContent = 'Error: ' + msg.data.error;
        statusEl.className = 'text-xs text-red-500';
    });

    function appendMessage(role, content) {
        var placeholder = document.getElementById('chat-placeholder');
        if (placeholder) placeholder.remove();

        var div = document.createElement('div');
        div.className = 'chat-msg ' + (role === 'user' ? 'text-cyan-700' : 'text-green-700');
        div.innerHTML = '<span class="font-bold">' + (role === 'user' ? 'You' : 'Claude') + ':</span> ' +
            '<span class="chat-content whitespace-pre-wrap">' + escapeHtml(content) + '</span>';
        history.appendChild(div);
        history.scrollTop = history.scrollHeight;
        return div;
    }

    function setStreaming(on) {
        streaming = on;
        streamingEl.className = on ? 'text-sm text-gray-500 mb-2' : 'hidden text-sm text-gray-500 mb-2';
        sendBtn.disabled = on;
        input.disabled = on;
        statusEl.textContent = on ? 'Streaming...' : 'Ready';
        statusEl.className = on ? 'text-xs text-blue-500' : 'text-xs text-gray-400';
    }

    function escapeHtml(str) {
        var div = document.createElement('div');
        div.textContent = str;
        return div.innerHTML;
    }

    function truncate(str, max) {
        return str.length > max ? str.substring(0, max) + '...' : str;
    }
})();
