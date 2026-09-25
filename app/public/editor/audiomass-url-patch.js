// Add this to app.js after line 78 (after the local= parameter check)

// Check for audioUrl parameter to load audio from URL
if (w.location.href.split('audioUrl=')[1]) {
    var audioUrl = decodeURIComponent(w.location.href.split('audioUrl=')[1].split('&')[0]);

    setTimeout(function () {
        if (audioUrl) {
            q.engine.LoadURL(audioUrl);
        }
    }, 500);
}
