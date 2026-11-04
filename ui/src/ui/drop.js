// Takes the OS drop over the notch's drop zone (shown only while Badi waits for a folder or files):
// tells main when something is dragged over, so the notch lights up, and hands over the paths.
(function () {
  let depth = 0; // dragenter/leave fire for children too
  const hover = (on) => badi.send('drop:hover', on);
  document.addEventListener('dragenter', (e) => {
    e.preventDefault();
    if (!depth++) hover(true);
  });
  document.addEventListener('dragleave', () => {
    depth = Math.max(0, depth - 1);
    if (!depth) hover(false);
  });
  document.addEventListener('dragover', (e) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'copy';
  });
  document.addEventListener('drop', (e) => {
    e.preventDefault();
    depth = 0;
    hover(false);
    badi.send('drop:files', badi.pathsOf(e.dataTransfer.files));
  });
})();
