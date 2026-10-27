(function(){
  const nav = document.querySelector('[data-nav]');
  const toggle = document.querySelector('[data-nav-toggle]');
  if(toggle && nav){ toggle.addEventListener('click',()=>{let expanded=toggle.getAttribute('aria-expanded')==='true';toggle.setAttribute('aria-expanded',String(!expanded));nav.classList.toggle('open',!expanded);}); }
  document.querySelectorAll('[data-demo-form]').forEach(form=>form.addEventListener('submit',ev=>{
    ev.preventDefault();const msg=form.querySelector('[data-form-message]');if(msg){msg.textContent=document.documentElement.lang==='es'?'Demostración: no se envió ningún mensaje.':'Demo only: nothing was sent.';msg.className='notice notice-success';}
  }));
  document.querySelectorAll('[data-not-live]').forEach(link=>link.addEventListener('click',ev=>{ev.preventDefault();const el=document.querySelector('#global-status');if(el)el.textContent='Prototype only — texting is not connected. Use Start online to try the demo.';}));
})();
