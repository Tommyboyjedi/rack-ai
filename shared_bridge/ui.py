"""The single operator page: pick a service, activate it, see the current grant."""

PAGE = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>GPU Rack — Shared Service</title>
<style>
  body{font:14px system-ui,sans-serif;max-width:520px;margin:4vh auto;padding:1rem;background:#0d1117;color:#c9d1d9}
  h1{font-size:1.25rem;margin:0 0 1rem}
  .status{padding:.5rem .75rem;border-radius:6px;margin-bottom:1rem;font-size:.85rem;background:#161b22;border:1px solid #30363d}
  .status.err{border-color:#5a1e1e}
  select{width:100%;padding:.5rem;background:#0d1117;color:#c9d1d9;border:1px solid #30363d;border-radius:6px;font-size:1rem}
  button{margin-top:.75rem;padding:.5rem 1rem;background:#238636;color:#fff;border:none;border-radius:6px;cursor:pointer;font-size:.95rem}
  button:disabled{opacity:.5;cursor:default}
  .actions{display:flex;gap:.5rem;flex-wrap:wrap}
  #release{background:#da3633}
  .info{font-size:.75rem;color:#8b949e;margin-top:.75rem;white-space:pre-wrap}
  .current{margin-top:1rem;padding:.6rem .75rem;border-radius:6px;border:1px solid #30363d;background:#11151b}
  .current b{color:#c9d1d9}
</style>
</head>
<body>
<h1>GPU Rack — Shared Service</h1>
<div id="status" class="status">Loading…</div>
<label for="sel">Active service</label>
<select id="sel"><option value="">— loading —</option></select>
<div class="actions">
<button id="btn" disabled>Activate / change service</button>
<button id="release" title="Release the reservation, unload the resident service and keep the bridge off until activated." disabled>Release reservation</button>
</div>
<div class="current" id="current">Current reservation: <b>none</b></div>
<div class="info" id="info"></div>
<script>
(function(){
  const sel=document.getElementById('sel'),btn=document.getElementById('btn'),
        statusEl=document.getElementById('status'),info=document.getElementById('info'),
        currentEl=document.getElementById('current'),releaseBtn=document.getElementById('release');
  let busy=false,lastStatus='Ready.';
  function show(text,err){statusEl.className='status'+(err?' err':'');statusEl.textContent=text}
  function render(d){
    lastStatus=d.enabled===false
      ?(d.switch?'Bridge off. Waiting for the service to unload ('+d.switch.phase+').':'Bridge off. Activate a service to resume.')
      :(d.switch?'Changing service ('+d.switch.phase+').':'Ready.');
    const names=(d.services||[]).map(s=>s.service);
    const previous=sel.value;
    sel.innerHTML='';
    for(const name of names){
      const option=document.createElement('option');
      option.value=name;option.textContent=name+(name===d.selected_service?' ✓':'');
      sel.appendChild(option);
    }
    if(names.includes(d.selected_service)){sel.value=d.selected_service}
    else if(names.includes(previous)){sel.value=previous}
    btn.disabled=busy||!!d.switch||names.length===0;
    releaseBtn.disabled=busy||(d.enabled===false&&!d.reservation&&!d.switch);
    const r=d.reservation;
    currentEl.innerHTML='Current reservation: <b>'+(r?(r.service+' ('+(r.state||'unknown')+')'):'none')+'</b>'
      +(r?('<br><small>'+r.id+'</small>'):'');
    const sw=d.switch?(' • switching to '+d.switch.to_service+' ('+d.switch.phase+')'):'';
    const out=d.outcome?(' • last result: '+d.outcome.state+(d.outcome.reason?' ('+d.outcome.reason+')':'')):'';
    info.textContent='Selected: '+d.selected_service+sw+out
      +'\\nServices: '+names.join(', ');
  }
  async function poll(){
    if(busy){return}
    try{
      const response=await fetch('status',{cache:'no-store'});
      if(!response.ok){throw new Error('status '+response.status)}
      render(await response.json());
      show(lastStatus);
    }catch(error){show('Bridge unavailable: '+error.message,true)}
  }
  btn.onclick=async function(){
    const service=sel.value;if(!service||busy){return}
    busy=true;btn.disabled=true;releaseBtn.disabled=true;show('Switching to '+service+'…');
    try{
      const response=await fetch('select',{method:'POST',headers:{'Content-Type':'application/json'},
        body:JSON.stringify({service:service})});
      const body=await response.json();
      if(!response.ok){throw new Error(body.error||('status '+response.status))}
      render(body);show('Selected '+service+'.');
    }catch(error){show('Switch failed: '+error.message,true)}
    finally{busy=false;poll()}
  };
  releaseBtn.onclick=async function(){
    if(busy){return}
    busy=true;btn.disabled=true;releaseBtn.disabled=true;show('Releasing reservation and unloading service…');
    try{
      const response=await fetch('release',{method:'POST',headers:{'Content-Type':'application/json'},body:'{}'});
      const body=await response.json();
      if(!response.ok){throw new Error(body.error||('status '+response.status))}
      render(body);
    }catch(error){show('Release failed: '+error.message,true)}
    finally{busy=false;poll()}
  };
  poll();setInterval(poll,3000);
})();
</script>
</body>
</html>
"""
