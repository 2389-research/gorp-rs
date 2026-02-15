// ABOUTME: Feed page WebSocket client for real-time message display
// ABOUTME: Subscribes to feed channel and appends server-rendered HTML fragments

(function() {
    var container = document.getElementById('feed-messages');
    var filter = document.getElementById('platform-filter');
    var status = document.getElementById('connection-status');

    // Subscribe to feed + status channels
    window.gorp.subscribe(['feed', 'status']);

    // Update connection indicator when socket connects
    window.gorp.on('status.platform', function(msg) {
        status.textContent = msg.data.platform + ': ' + msg.data.state;
    });

    // Append new feed messages
    window.gorp.on('feed.message', function(msg) {
        var placeholder = container.querySelector('p.text-gray-400');
        if (placeholder) placeholder.remove();

        var div = document.createElement('div');
        div.innerHTML = msg.html;
        var el = div.firstElementChild;
        if (el) {
            el.setAttribute('data-platform', msg.data.platform);
            container.appendChild(el);
            applyFilter();
            // Auto-scroll to bottom
            container.scrollTop = container.scrollHeight;
        }
    });

    // Platform filter
    filter.addEventListener('change', applyFilter);

    function applyFilter() {
        var platform = filter.value;
        var msgs = container.querySelectorAll('.feed-msg');
        for (var i = 0; i < msgs.length; i++) {
            if (!platform || msgs[i].getAttribute('data-platform') === platform) {
                msgs[i].style.display = '';
            } else {
                msgs[i].style.display = 'none';
            }
        }
    }

    status.textContent = 'Connected';
})();
