/* Passive operator energy display. All telemetry labels/content use text nodes. */
const energySection=document.querySelector('#energy');
if(energySection){
 const period=document.querySelector('#energy-period'),body=document.querySelector('#energy-rows');
 const message=document.querySelector('#energy-status'),assumptions=document.querySelector('#energy-assumptions');
 const chart=document.querySelector('#energy-chart'),legend=document.querySelector('#energy-legend');
 const colours=['#ffbf69','#7dd3fc','#a7f3d0','#c4b5fd','#fca5a5','#f9a8d4'];
 let snapshot=null,loading=false;
 const number=(n,d=2)=>typeof n==='number'&&Number.isFinite(n)?n.toLocaleString(undefined,{maximumFractionDigits:d,minimumFractionDigits:d}):'Unavailable';
 const kind=s=>s.classification==='estimated'?'Estimated — uncalibrated':s.classification==='counter_derived'?'CPU package counter':'Measured GPU telemetry';
 function node(tag,text){const n=document.createElement(tag);n.textContent=text;return n;}
 function svg(tag,attrs){const n=document.createElementNS('http://www.w3.org/2000/svg',tag);for(const [k,v] of Object.entries(attrs))n.setAttribute(k,v);return n;}
 function render(){
  if(!snapshot)return;
  const history=snapshot.history[period.value],latest=snapshot.latest;
  const live=new Map(latest.gpus.map(g=>[g.id,g.watts]));
  live.set('estimated-total',latest.estimated_total_watts);live.set('cpu-package',latest.cpu_package_watts);
  const series=[...history.series].sort((a,b)=>(a.id==='estimated-total'?-1:b.id==='estimated-total'?1:a.label.localeCompare(b.label)));
  body.replaceChildren();
  for(const s of series){
   const row=document.createElement('tr');
   const label=node('th',s.label);label.scope='row';row.append(label);
   for(const text of [kind(s),number(live.get(s.id),1)+' W',(s.coverage_seconds>0?number(s.kwh,4)+' kWh':'Unavailable'),number(s.coverage_seconds/3600,2)+' h'])row.append(node('td',text));
   body.append(row);
  }
  if(!series.length)body.append(node('tr','Gathering the first energy interval…'));
  const gpuSeries=series.filter(s=>s.classification==='measured_device'&&s.coverage_seconds>0);
  const gpuEnergy=gpuSeries.length?gpuSeries.reduce((sum,s)=>sum+s.kwh,0):null;
  document.querySelector('#energy-gpu-total').textContent='Combined GPU energy in this period: '+number(gpuEnergy,4)+' kWh (recorded intervals).';
  const m=snapshot.methodology;
  assumptions.textContent='Estimated rack total = (GPU + CPU package + '+number(m.other_components_watts,0)+' W assumed for RAM, drives, motherboard and fans) ÷ '+number(m.psu_efficiency*100,0)+'% assumed PSU efficiency. These are provisional, uncalibrated assumptions. This is not a measured socket total.';
  message.textContent=(snapshot.stale?'STALE — collector has not updated recently. ':'')+'Latest sample: '+new Date(snapshot.collected_at*1000).toLocaleString()+'. History began '+new Date(snapshot.started_at*1000).toLocaleString()+'. '+snapshot.warnings.join('. ');
  draw(history,series);
 }
 function draw(history,series){
  chart.replaceChildren();legend.replaceChildren();
  const left=55,top=18,width=700,height=220;
  let max=1;
  for(const s of series)for(const p of s.points)if(p.seconds>0)max=Math.max(max,p.kwh*3600000/p.seconds);
  max=Math.ceil(max/10)*10;
  chart.append(svg('path',{d:'M55 18 V238 H755',stroke:'#aabccc',fill:'none'}));
  const axis=svg('text',{x:3,y:16,fill:'#e5f2ff','font-size':12});axis.textContent=max+' W';chart.append(axis);
  series.forEach((s,index)=>{
   const colour=colours[index%colours.length],entry=node('li',s.label+' — '+kind(s));entry.style.color=colour;legend.append(entry);
   let points=[],previous=null;
   function segment(){
    if(points.length){chart.append(svg('polyline',{points:points.join(' '),fill:'none',stroke:colour,'stroke-width':2,'stroke-dasharray':s.classification==='estimated'?'7 4':'none'}));}
    points=[];
   }
   for(const p of s.points){
    if(p.seconds<=0)continue;
    if(previous!==null&&p.at-previous>history.bucket_seconds*1.01)segment();
    const x=left+(p.at-history.start)/(history.end-history.start)*width,y=top+height-(p.kwh*3600000/p.seconds)/max*height;
    points.push(x.toFixed(2)+','+y.toFixed(2));
    chart.append(svg('circle',{cx:x,cy:y,r:2,fill:colour}));
    previous=p.at;
   }
   segment();
  });
  const range=svg('text',{x:left,y:265,fill:'#e5f2ff','font-size':12});range.textContent=new Date(history.start*1000).toLocaleString()+' — '+new Date(history.end*1000).toLocaleString();chart.append(range);
 }
 async function refreshEnergy(){
  if(loading)return;loading=true;
  try{
   const status=await fetch('/api/media/v1/status').then(r=>r.json());
   energySection.hidden=!(status.operator&&status.energy_supported);
   if(energySection.hidden)return;
   const response=await fetch('/api/media/v1/energy');
   if(!response.ok)throw Error('Energy history is temporarily unavailable.');
   snapshot=await response.json();render();
  }catch(error){message.textContent=error.message+' Previously displayed readings may be stale.';}finally{loading=false;}
 }
 period.onchange=render;
 document.querySelector('#energy-refresh').onclick=refreshEnergy;
 refreshEnergy();setInterval(refreshEnergy,30000);
}
