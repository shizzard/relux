(function () {
  function ready(fn) {
    if (document.readyState !== "loading") fn();
    else document.addEventListener("DOMContentLoaded", fn);
  }

  ready(function () {
    var input = document.querySelector('input[data-search-input]');
    if (!input) return;
    var counter = document.querySelector('.search-input .count');
    var kbdBadge = document.querySelector('.search-input .kbd');
    var isMac = /Mac|iPod|iPhone|iPad/.test(navigator.platform);
    if (kbdBadge) kbdBadge.textContent = isMac ? "\u2318S" : "Ctrl+S";

    var groups = Array.prototype.slice.call(document.querySelectorAll('.group'));
    var groupHeaders = Array.prototype.slice.call(document.querySelectorAll('.group-header'));
    var rows = Array.prototype.slice.call(document.querySelectorAll('.row'));

    rows.forEach(function (row) {
      var nameSpan = row.querySelector('.test .name');
      if (nameSpan) row.dataset.originalName = nameSpan.textContent;
    });

    var currentIndex = -1;
    var currentHits = [];

    function escapeRegex(s) {
      return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    }

    function clearHighlights() {
      rows.forEach(function (row) {
        var nameSpan = row.querySelector('.test .name');
        if (nameSpan && row.dataset.originalName !== undefined) {
          nameSpan.textContent = row.dataset.originalName;
        }
      });
    }

    function rebuildHits(query) {
      clearHighlights();
      if (query.length === 0) {
        rows.forEach(function (row) { row.hidden = false; });
        groups.forEach(function (g) { g.hidden = false; });
        groupHeaders.forEach(function (h) {
          h.hidden = false;
          var label = h.dataset.groupHeader;
          var count = document.querySelectorAll('.group[data-group="' + label + '"] .row').length;
          h.innerHTML = '\u2014 ' + label + ' <span class="count">(' + count + ')</span>';
        });
        if (counter) counter.textContent = "";
        return [];
      }

      var insensitive = query === query.toLowerCase();
      var flags = insensitive ? "gi" : "g";
      var re = new RegExp(escapeRegex(query), flags);

      var hits = [];
      var perGroupVisible = {};
      var perGroupTotal = {};
      rows.forEach(function (row) {
        var groupEl = row.closest('.group');
        if (!groupEl) return;
        var groupLabel = groupEl.dataset.group;
        if (!(groupLabel in perGroupTotal)) perGroupTotal[groupLabel] = 0;
        if (!(groupLabel in perGroupVisible)) perGroupVisible[groupLabel] = 0;
        perGroupTotal[groupLabel]++;

        var name = row.dataset.originalName || "";
        re.lastIndex = 0;
        var matches = [];
        for (var m = re.exec(name); m !== null; m = re.exec(name)) {
          matches.push({ start: m.index, end: m.index + m[0].length });
          if (m.index === re.lastIndex) re.lastIndex++;
        }
        if (matches.length === 0) {
          row.hidden = true;
          return;
        }
        row.hidden = false;
        perGroupVisible[groupLabel]++;

        var nameSpan = row.querySelector('.test .name');
        if (!nameSpan) return;
        var frag = document.createDocumentFragment();
        var pos = 0;
        matches.forEach(function (mt) {
          if (mt.start > pos) frag.appendChild(document.createTextNode(name.slice(pos, mt.start)));
          var mark = document.createElement('mark');
          mark.className = 'search-hit';
          mark.textContent = name.slice(mt.start, mt.end);
          frag.appendChild(mark);
          hits.push({ row: row, mark: mark });
          pos = mt.end;
        });
        if (pos < name.length) frag.appendChild(document.createTextNode(name.slice(pos)));
        nameSpan.textContent = '';
        nameSpan.appendChild(frag);
      });

      groupHeaders.forEach(function (h) {
        var label = h.dataset.groupHeader;
        var visible = perGroupVisible[label] || 0;
        var total = perGroupTotal[label] || 0;
        var groupEl = document.querySelector('.group[data-group="' + label + '"]');
        if (visible === 0) {
          h.hidden = true;
          if (groupEl) groupEl.hidden = true;
        } else {
          h.hidden = false;
          if (groupEl) groupEl.hidden = false;
          h.innerHTML = '\u2014 ' + label + ' <span class="count">(' + visible + ' / ' + total + ')</span>';
        }
      });

      if (counter) counter.textContent = hits.length + " / " + rows.length;
      return hits;
    }

    function markCurrent(index) {
      currentHits.forEach(function (h) { h.mark.classList.remove('search-hit-current'); });
      if (index < 0 || index >= currentHits.length) return;
      var hit = currentHits[index];
      hit.mark.classList.add('search-hit-current');
      var rect = hit.row.getBoundingClientRect();
      var top = window.scrollY + rect.top + rect.height / 2 - window.innerHeight / 2;
      var max = document.documentElement.scrollHeight - window.innerHeight;
      window.scrollTo(0, Math.max(0, Math.min(max, top)));
    }

    function recompute() {
      currentHits = rebuildHits(input.value);
      currentIndex = currentHits.length > 0 ? 0 : -1;
      markCurrent(currentIndex);
    }

    input.addEventListener('input', recompute);
    input.addEventListener('keydown', function (event) {
      if (event.key === 'Enter') {
        event.preventDefault();
        if (currentHits.length === 0) return;
        var delta = event.shiftKey ? -1 : 1;
        currentIndex = (currentIndex + delta + currentHits.length) % currentHits.length;
        markCurrent(currentIndex);
      } else if (event.key === 'Escape') {
        event.preventDefault();
        if (input.value.length > 0) {
          input.value = '';
          recompute();
        } else {
          input.blur();
        }
      }
    });

    document.addEventListener('keydown', function (event) {
      if ((event.metaKey || event.ctrlKey) && !event.altKey && !event.shiftKey && event.key.toLowerCase() === 's') {
        event.preventDefault();
        input.focus();
        input.select();
      }
    });
  });
})();
