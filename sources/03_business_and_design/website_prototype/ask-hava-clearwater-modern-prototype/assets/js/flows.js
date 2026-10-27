/* Front-end demonstration only. No network requests and no actual text messages. */
(function(){
  const D=window.AskHavaSamples;
  const $=(s,p=document)=>p.querySelector(s);
  const $$=(s,p=document)=>Array.from(p.querySelectorAll(s));
  const lang=document.documentElement.lang==='es'?'es':'en';
  const tr=(en,es)=>lang==='es'?es:en;
  const announce=(message)=>{const x=$('#flow-status');if(x)x.textContent=message;};
  const show=(el)=>{if(el)el.hidden=false;}; const hide=el=>{if(el)el.hidden=true;};
  const escape=s=>String(s).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  function initRequest(){
    const root=$('#request-app');if(!root)return;
    const panes=$$('[data-step]',root);let step=0;let danger=false;
    const progress=$('#request-progress');
    const params=new URLSearchParams(window.location.search);
    const requestedCategory=params.get('category'); const select=$('#request-category');
    if(requestedCategory&&select&&Array.from(select.options).some(o=>o.value===requestedCategory)){select.value=requestedCategory;}
    const repeat=params.get('repeat');if(repeat&&$('#repeat-note')){$('#repeat-note').hidden=false;$('#repeat-note').textContent=tr('Request a repeat visit from ','Solicita otra visita de ')+repeat+' (sample request only)';}
    function nextStep(n){panes.forEach((p,i)=>p.hidden=i!==n);step=n;progress.innerHTML=panes.map((p,i)=>`<span class="dot ${i===n?'active':''}">${i+1} ${i===n?'•':''}</span>`).join('');const hd=$('h2',panes[n]);if(hd){hd.tabIndex=-1;hd.focus();}announce(tr('Step '+(n+1)+' of '+panes.length,'Paso '+(n+1)+' de '+panes.length));}
    root.addEventListener('click',e=>{
      const next=e.target.closest('[data-next]');const prev=e.target.closest('[data-prev]');
      if(prev){nextStep(Math.max(0,step-1));return;}
      if(next){let current=panes[step];let missing=$$('[data-required]',current).find(x=>!x.value.trim());
        if(missing){missing.setAttribute('aria-invalid','true');const err=$('.error',current);if(err)err.textContent=tr('Please complete the required fields.','Completa los campos obligatorios.');missing.focus();return;}
        if(step===1){const problem=$('#request-problem').value.toLowerCase();danger=/(gas smell|rotten egg|sparks|smoke|burning|flood|olor a gas|huevo podrido|chispas|humo|inundaci[oó]n)/.test(problem);if(danger){show($('#safety-branch'));$('#safety-branch').scrollIntoView({block:'nearest'});announce(tr('Possible emergency. Read safety guidance first.','Posible emergencia. Lee el aviso de seguridad.'));return;}}
        nextStep(Math.min(panes.length-1,step+1));
      }
    });
    const safetyProceed=$('#safety-continue');if(safetyProceed)safetyProceed.addEventListener('click',()=>{
      const safe=$('input[name=safe]:checked');const urgency=$('input[name=urgent]:checked');
      if(!safe||!urgency){announce(tr('Answer the safety questions first.','Responde primero las preguntas de seguridad.'));return;}
      if(safe.value==='no'){announce(tr('Please move to safety and call 911 before continuing.','Ponte a salvo y llama al 911 antes de continuar.'));return;}
      hide($('#safety-branch'));nextStep(2);
    });
    $('#request-ownership')?.addEventListener('change',e=>{const x=$('#onsite-wrap');x.hidden=e.target.value==='owner'||!e.target.value;});
    $('#request-photo')?.addEventListener('change',e=>{const f=e.target.files?.[0];$('#photo-name').textContent=f?tr('Selected locally (not uploaded): ','Seleccionado localmente (no enviado): ')+f.name:'';});
    $('#request-submit')?.addEventListener('click',()=>{const zip=$('#request-zip');if(!/^\d{5}$/.test(zip.value)){zip.setAttribute('aria-invalid','true');zip.focus();announce(tr('Enter a five-digit ZIP code.','Escribe un código postal de cinco dígitos.'));return;}nextStep(3);});
    nextStep(0);
  }
  function modal(dialog,focusSelector){
    let last=document.activeElement;dialog.showModal();const target=focusSelector?$(focusSelector,dialog):$('button,input,textarea',dialog);target?.focus();
    dialog.addEventListener('close',()=>last?.focus(),{once:true});
  }
  function initQuotes(){
    const root=$('#quotes-app');if(!root)return;const list=$('#quotes-list');const recurring=$('#recurring-list');let selected=null;
    D.quotes.forEach((q,i)=>{
      const price=q.type==='range'?`$${q.min}–$${q.max}`:`$${q.price}${q.type==='monthly'?tr(' / month',' / mes'):''}`;
      const license=q.licenseRequired?`${escape(q.license)} ${tr('shows active on the state website as of 10/26/2026 (sample wording only; no verification was performed).','aparece activa en el sitio del estado al 26/10/2026 (texto de muestra; no se verificó).')}`:tr('No contractor license claim is made for this example.','No se declara licencia de contratista para este ejemplo.');
      const note=q.type==='trip'?`<p class="error">${tr('This is not a repair price.','Esto no es el precio de la reparación.')}</p>`:'';
      const card=document.createElement('article');card.className='quote-card';card.innerHTML=`<header><div><span class="tag">${tr('Sample business','Negocio ficticio')}</span><h3>${escape(q.name)}</h3></div><small>${tr(q.received,'Recibida '+(i+1)+'ª')} · ${tr('Arrival order','Orden de llegada')}</small></header>
      <p class="fine">${tr('Independent business, not affiliated with Ask Hava.','Empresa independiente, no afiliada a Ask Hava.')}</p>
      <div class="quote-kind">${escape(q.kind)}</div><div class="quote-price">${price}</div>${note}
      <p class="quote-words">“${escape(q.words)}”</p><dl class="quote-meta"><div><dt>${tr('Included','Incluye')}</dt><dd>${escape(q.includes)}</dd></div><div><dt>${tr('Earliest visit','Primera visita')}</dt><dd>${escape(q.arrival)}</dd></div><div><dt>${tr('Quote good until','Válida hasta')}</dt><dd>${escape(q.expires)}</dd></div><div><dt>${tr('License example','Ejemplo de licencia')}</dt><dd>${license}</dd></div></dl>
      <p class="fine"><a href="https://azroc.my.site.com/AZRoc/s/contractor-search" target="_blank" rel="noopener">${tr('Check it yourself — Arizona ROC','Compruébala en Arizona ROC')}</a> · <a href="#review-note" class="reviews-demo">${tr('Read their reviews (demo placeholder)','Ver reseñas (ejemplo)')}</a></p>
      <div class="pill-list">${q.spanish?'<span class="tag tag-soft">'+tr('We speak Spanish','Hablamos español')+'</span>':''}${q.afterHours?'<span class="tag tag-soft">'+tr('After-hours available','Horario extendido')+'</span>':''}</div>
      <div class="button-row" style="margin-top:1.3rem"><button class="btn" type="button" data-choose="${q.id}">${tr('Choose this quote','Elegir esta cotización')}</button></div>`;(q.type==='monthly'&&recurring?recurring:list).appendChild(card);
    });
    const choice=$('#choose-dialog');root.addEventListener('click',e=>{const b=e.target.closest('[data-choose]');if(b){selected=D.quotes.find(q=>q.id===b.dataset.choose);$('#chosen-business').textContent=selected.name;$('#demo-code').value='';$('#code-error').textContent='';modal(choice,'#demo-code');}});
    $('#choose-form')?.addEventListener('submit',e=>{e.preventDefault();const inp=$('#demo-code');if(inp.value.trim()!==D.demoCode){inp.setAttribute('aria-invalid','true');$('#code-error').textContent=tr('Incorrect demo code. Enter 123456.','Código de demostración incorrecto. Escribe 123456.');inp.focus();return;}
      inp.removeAttribute('aria-invalid');choice.close();hide($('#quotes-main'));show($('#quote-success'));$('#picked-name').textContent=selected.name;$('#quote-success h2').tabIndex=-1;$('#quote-success h2').focus();announce(tr('Choice confirmed in this demo. No text was sent.','Elección confirmada en esta demostración. No se envió ningún mensaje.'));
    });
    $$('.dialog-close').forEach(x=>x.addEventListener('click',()=>x.closest('dialog').close()));
    $$('[data-dialog]').forEach(b=>b.addEventListener('click',()=>modal($(b.dataset.dialog))));
    $('#close-request')?.addEventListener('click',()=>{modal($('#close-dialog'));});
    $('#confirm-close')?.addEventListener('click',()=>{$('#close-dialog').close();$('#quotes-main').innerHTML='<div class="notice">'+tr('Sample request closed. Nothing was changed on a server.','Solicitud de ejemplo cerrada. No se modificó ningún servidor.')+'</div>';announce(tr('Request closed in the demo.','Solicitud cerrada en la demostración.'));});
    $$('[data-demo-action]').forEach(b=>b.addEventListener('click',()=>{const d=b.closest('dialog');d.close();announce(tr('Demo saved locally. Nothing was sent.','Demostración guardada localmente. No se envió nada.'));}));
    $('#reviews-placeholder')?.addEventListener('click',e=>e.preventDefault());
    root.addEventListener('click',e=>{if(e.target.closest('.reviews-demo')){e.preventDefault();announce(tr('Real review links will be provided by businesses in the live service.','Los enlaces a reseñas estarán disponibles en el servicio real.'));}});
  }
  function initPortal(){
    const root=$('#portal-app');if(!root)return;
    $('#signin-form')?.addEventListener('submit',e=>{e.preventDefault();const code=$('#portal-code');if(code.value!==D.demoCode){$('#signin-error').textContent=tr('Use 123456 for this demo.','Usa 123456 en esta demostración.');code.focus();return;}hide($('#portal-signin'));show($('#portal-content'));announce(tr('Demo account opened.','Cuenta de demostración abierta.'));$('#portal-content h2').tabIndex=-1;$('#portal-content h2').focus();});
    $$('.portal-tab',root).forEach(b=>b.addEventListener('click',()=>{$$('.portal-tab',root).forEach(x=>{x.setAttribute('aria-selected',String(x===b));});$$('[data-portal-panel]',root).forEach(x=>x.hidden=x.id!==b.dataset.target);$('#'+b.dataset.target+' h3')?.focus();}));
    $$('.local-save',root).forEach(b=>b.addEventListener('click',()=>announce(tr('Saved for this demo only. No server was contacted.','Guardado solo para esta demostración. No se contactó ningún servidor.'))));
    $('#delete-demo')?.addEventListener('click',()=>{if(window.confirm(tr('Reset this demo view? No real data exists.','¿Reiniciar esta vista de demostración? No existen datos reales.'))){hide($('#portal-content'));show($('#portal-signin'));announce(tr('Demo view reset.','Vista reiniciada.'));}});
  }
  function initSignup(){
    const root=$('#signup-app');if(!root)return;let idx=0;const steps=$$('[data-signup-step]',root);
    const switchStep=n=>{idx=n;steps.forEach((s,i)=>s.hidden=i!==n);$('#signup-progress').textContent=`${tr('Step','Paso')} ${n+1} / ${steps.length}`;$('h2',steps[n])?.focus();};
    $('#lookup-license')?.addEventListener('click',()=>{const code=$('#license-number').value.trim();const status=$('#license-status');if(code==='000000'){status.className='notice notice-success';status.textContent='SAMPLE LOOKUP — R-39 (Air conditioning and refrigeration). It covers AC repair, heating and system replacement. This is fake data; no state lookup happened.';$('#license-lookup-ok').value='yes';}else{status.className='notice notice-danger';status.textContent='Demo only: enter 000000 to show the sample license lookup.';$('#license-lookup-ok').value='';}});
    $('#license-type')?.addEventListener('change',e=>{const none=e.target.value==='none';$('#license-wrap').hidden=none;$('#unlicensed-note').hidden=!none;$('#licensed-work').hidden=none;$('#unlicensed-work').hidden=!none;$$('input[name=work]',none?$('#licensed-work'):$('#unlicensed-work')).forEach(x=>x.checked=false);});
    root.addEventListener('click',e=>{if(e.target.closest('[data-signup-prev]'))switchStep(Math.max(0,idx-1));if(e.target.closest('[data-signup-next]')){
      if(idx===0){const name=$('#biz-name');const license=$('#license-type');if(!name.value.trim()){name.focus();announce('Enter your business name.');return;}if(license.value!=='none'&&$('#license-lookup-ok').value!=='yes'){announce('Use the sample license number 000000 and look it up first.');$('#license-number').focus();return;}}
      if(idx===1){const checked=$$('input[name=work]:checked');if(!checked.length){announce('Choose at least one service.');return;}if($('#license-type').value!=='none'&&checked.some(x=>x.dataset.covered==='no')){announce('That job type needs a different license. Remove it to continue.');return;}}
      if(idx===3){if($('#identity-code').value!=='123456'){announce('Enter the sample identity code 123456. No real code was sent.');$('#identity-code').focus();return;}if(!$('#business-consent').checked){announce('Texting consent is required for job notices. The box starts unchecked.');$('#business-consent').focus();return;}}
      switchStep(Math.min(steps.length-1,idx+1));
    }});
    $('#biz-identity')?.addEventListener('change',e=>{$('#identity-method-label').textContent={sms:'We will text the number on the state record (demo only).',call:'We will call the number on the state record (demo only).',email:'We will email the address on the state record (demo only).',mail:'We will mail a letter to the state-record address (demo only).'}[e.target.value];});
    switchStep(0);
  }
  function initBusiness(){
    const root=$('#business-app');if(!root)return;
    $$('.business-tab',root).forEach(b=>b.addEventListener('click',()=>{$$('.business-tab',root).forEach(x=>x.setAttribute('aria-selected',String(b===x)));$$('[data-business-panel]',root).forEach(p=>p.hidden=p.id!==b.dataset.target);}));
    const kind=$('#business-price-type');const recalc=()=>{if(!kind)return;const a=parseFloat($('#business-amount').value)||0;const b=parseFloat($('#business-max').value)||a;const middle=kind.value==='range'?(a+b)/2:kind.value==='monthly'?a*2:a;const fee=kind.value==='trip'?75:middle<500?25:middle<=1500?75:199;$('#live-tier').textContent=`$${fee} (demo estimate)`;$('#max-field').hidden=kind.value!=='range';};kind?.addEventListener('change',recalc);$('#business-amount')?.addEventListener('input',recalc);$('#business-max')?.addEventListener('input',recalc);recalc();
    $('#quote-submit')?.addEventListener('click',()=>{if(!$('#business-amount').value){$('#business-amount').focus();announce('Enter an amount.');return;}announce('Sample quote saved. No homeowner was contacted.');});
    $$('.confirm-pick',root).forEach(b=>b.addEventListener('click',()=>{b.disabled=true;b.textContent='Confirmed in demo';announce('Sample selection confirmed. In production the fee would be charged and masked calling activated.');}));
    $$('.business-save',root).forEach(b=>b.addEventListener('click',()=>announce('Business demo settings saved on this page only.')));
  }
  function initOwner(){
    const root=$('#owner-app');if(!root)return;$$('input[type=checkbox]',root).forEach(x=>x.addEventListener('change',()=>announce(`${x.dataset.label}: ${x.checked?'Paused in the demo':'Running in the demo'}. No production system affected.`)));
  }
  initRequest();initQuotes();initPortal();initSignup();initBusiness();initOwner();
})();
