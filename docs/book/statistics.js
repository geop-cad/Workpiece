// The site's anonymous usage statistics in the book's pages: loads website/analytics.js, which
// the site serves at its root, one level above the book (see the site's privacy page). That file
// sends nothing unless the deploy set a key, nor under Do Not Track or Global Privacy Control.
(function () {
  if (typeof path_to_root !== "string") return;
  var s = document.createElement("script");
  s.src = path_to_root + "../analytics.js";
  s.defer = true;
  document.head.appendChild(s);
})();
