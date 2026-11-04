const q = document.getElementById('q');
document.getElementById('f').onsubmit = (e) => {
  e.preventDefault();
  if (q.value.trim()) badi.invoke('ask', q.value.trim());
};
addEventListener('keydown', (e) => e.key === 'Escape' && window.close());
q.focus();
