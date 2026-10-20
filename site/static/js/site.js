const toggle = document.querySelector('.menu-toggle');
const sidebar = document.querySelector('#sidebar');
if (toggle && sidebar) {
  toggle.addEventListener('click', () => {
    const open = toggle.getAttribute('aria-expanded') !== 'true';
    toggle.setAttribute('aria-expanded', String(open));
    sidebar.classList.toggle('is-open', open);
  });
  sidebar.addEventListener('click', event => {
    if (event.target.closest('a')) {
      toggle.setAttribute('aria-expanded', 'false');
      sidebar.classList.remove('is-open');
    }
  });
}
