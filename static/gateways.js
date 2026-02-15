// ABOUTME: Live gateway status updates via WebSocket
// ABOUTME: Subscribes to status channel and updates gateway cards without page reload

(function() {
    if (!window.gorp) return;

    window.gorp.subscribe(['status']);

    window.gorp.on('status.platform', function(msg) {
        var platform = msg.data.platform;
        var state = msg.data.state;
        var card = document.getElementById('gw-' + platform);
        if (!card) return;

        var isConnected = (state === 'connected');
        var configured = card.dataset.configured === 'true';

        // Update card border/background
        if (isConnected) {
            card.className = card.className
                .replace('border-gray-200', '')
                .replace('border-green-200', '')
                .replace('bg-green-50', '');
            card.classList.add('border-green-200', 'bg-green-50');
        } else {
            card.className = card.className
                .replace('border-green-200', '')
                .replace('bg-green-50', '');
            card.classList.add('border-gray-200');
        }

        // Update indicator (filled/hollow circle)
        var indicator = card.querySelector('.gw-indicator');
        if (indicator) {
            indicator.innerHTML = isConnected ? '&#9679;' : '&#9675;';
            indicator.className = 'gw-indicator text-lg ' +
                (isConnected ? 'text-green-500' : 'text-gray-400');
        }

        // Update status text
        var statusText = card.querySelector('.gw-status-text');
        if (statusText) {
            var text = isConnected ? 'Connected' :
                       (configured ? 'Disconnected' : 'Not configured');
            if (state === 'connecting') text = 'Connecting...';
            if (state === 'auth_required') text = 'Auth Required';
            if (state === 'rate_limited') text = 'Rate Limited';

            statusText.textContent = text;
            statusText.className = 'gw-status-text text-sm ' +
                (isConnected ? 'text-green-600' : 'text-gray-500');
        }
    });
})();
