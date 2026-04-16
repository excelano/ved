// Axe — Theme toggle
// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)
//
// Include in <head> to detect system preference and restore saved choice.
// Add a <button class="theme-toggle"> anywhere to let users switch themes.

(function() {
    // Detect and apply theme before paint
    var saved = localStorage.getItem('theme');
    if (saved) document.documentElement.setAttribute('data-theme', saved);
    else if (window.matchMedia('(prefers-color-scheme: dark)').matches)
        document.documentElement.setAttribute('data-theme', 'dark');

    // Bind toggle button after DOM is ready
    document.addEventListener('DOMContentLoaded', function() {
        var toggle = document.querySelector('.theme-toggle');
        if (!toggle) return;
        toggle.addEventListener('click', function() {
            var current = document.documentElement.getAttribute('data-theme');
            var next = current === 'dark' ? 'light' : 'dark';
            document.documentElement.setAttribute('data-theme', next);
            localStorage.setItem('theme', next);
        });
    });
})();
