//! Shared page layout template — ADR-015 §2 (T65 dark mode + mobile).

use askama::Template;

#[derive(Template)]
#[template(
    source = r#"<!DOCTYPE html>
<html lang="zh">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>llm_proxy dashboard</title>
<style>
:root{--bg:#f5f5f5;--card:#fff;--fg:#222;--muted:#777;--accent:#2563eb;--err:#dc2626;--brd:#ddd;--stat-bg:#f0f7ff}
@media (prefers-color-scheme:dark){:root{--bg:#111;--card:#1e1e1e;--fg:#eee;--muted:#999;--accent:#60a5fa;--err:#f87171;--brd:#333;--stat-bg:#1a2a3a}}
*{box-sizing:border-box;margin:0;padding:0}
body{font:14px/1.5 system-ui,sans-serif;background:var(--bg);color:var(--fg);padding:20px}
nav{display:flex;gap:12px;margin-bottom:20px;flex-wrap:wrap}
nav a{color:var(--accent);text-decoration:none;padding:4px 12px;border-radius:4px;font-size:13px}
nav a:hover,nav a.active{background:var(--accent);color:#fff}
.card{background:var(--card);border:1px solid var(--brd);border-radius:6px;padding:16px;margin-bottom:16px}
table{width:100%;border-collapse:collapse}
th,td{text-align:left;padding:6px 10px;border-bottom:1px solid var(--brd)}
th{font-weight:600;color:var(--muted);font-size:12px;text-transform:uppercase}
.muted{color:var(--muted)}
.err{color:var(--err)}
.num{font-variant-numeric:tabular-nums;text-align:right}
.spark{display:flex;gap:1px;align-items:flex-end;height:20px}
.spark-bar{width:3px;background:var(--accent);border-radius:1px}
/* T65 utility classes */
.stat-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:12px;margin-bottom:16px}
.stat{background:var(--card);border:1px solid var(--brd);border-radius:6px;padding:14px 16px;text-align:center}
.stat .stat-num{font-size:26px;font-weight:700;color:var(--accent)}
.stat .stat-label{font-size:11px;color:var(--muted);margin-top:4px;text-transform:uppercase}
.chart{background:var(--card);border:1px solid var(--brd);border-radius:4px}
.empty{color:var(--muted);font-style:italic;padding:12px 0}
@media(max-width:768px){
  body{padding:10px}
  nav{flex-direction:column;gap:4px}
  table{font-size:12px}
  th,td{padding:4px 6px}
  .stat-grid{grid-template-columns:repeat(2,1fr)}
  .stat .stat-num{font-size:20px}
}
/* T66: loading/error states */
.server-error{color:var(--err);font-size:16px;text-align:center;padding:40px 0}
<script>
// T67: SVG hover tooltip + crosshair (vanilla JS)
document.querySelectorAll('.chart').forEach(function(svg){
  var tip=document.createElement('div');
  tip.style.cssText='position:absolute;background:var(--card);border:1px solid var(--brd);padding:4px 8px;border-radius:4px;font-size:12px;pointer-events:none;display:none;z-index:10';
  svg.parentNode.appendChild(tip);
  svg.addEventListener('mousemove',function(e){
    var pt=svg.createSVGPoint();
    pt.x=e.clientX;pt.y=e.clientY;
    var p=pt.matrixTransform(svg.getScreenCTM().inverse());
    var ds=JSON.parse(svg.dataset.points||'[]');
    var best=null,bestD=99;
    ds.forEach(function(d,i){
      var dx=Math.abs(p.x-d.x),dy=Math.abs(p.y-d.y);
      if(dx<12&&dy<12&&dx+dy<bestD){bestD=dx+dy;best=d;}
    });
    if(best){tip.style.display='block';tip.style.left=(e.clientX+12)+'px';tip.style.top=(e.clientY-30)+'px';tip.textContent=best.label;}
    else{tip.style.display='none';}
  });
  svg.addEventListener('mouseleave',function(){tip.style.display='none';});
});
</script>
<script>
// T68: auto-polling 30s with pause + localStorage
(function(){
var pause=false;
try{var stored=localStorage.getItem('llm_proxy_poll_pause');if(stored==='1')pause=true;}catch(_){}
function tick(s){
  var btn=document.getElementById('poll-btn');
  if(!btn)return;
  if(pause){btn.textContent='▶ 继续轮询';return;}
  if(s<=0){location.reload();return;}
  btn.textContent='⏸ 暂停 ('+s+'s)';
  setTimeout(function(){tick(s-1)},1000);
}
var btn=document.getElementById('poll-btn');
if(btn){
  if(pause){btn.textContent='▶ 继续轮询';}
  else{tick(30);}
  btn.addEventListener('click',function(){
    pause=!pause;
    try{localStorage.setItem('llm_proxy_poll_pause',pause?'1':'0');}catch(_){}
    if(pause){btn.textContent='▶ 继续轮询';}
    else{tick(30);}
  });
}
})();
</script>
</style>
</head>
<body>
<nav>
    <a href="/admin"        class="{% if is_active_cost %}active{% endif %}">成本</a>
    <a href="/admin/requests" class="{% if is_active_requests %}active{% endif %}">请求</a>
    <a href="/admin/keys"     class="{% if is_active_keys %}active{% endif %}">Key</a>
    <a href="/admin/traffic"  class="{% if is_active_traffic %}active{% endif %}">流量</a>
    <a href="/admin/alerts"   class="{% if is_active_alerts %}active{% endif %}">告警</a>
    <a href="/admin/help"     class="{% if is_active_help %}active{% endif %}">帮助</a>
    <button id="poll-btn" style="margin-left:auto;padding:4px 10px;border:1px solid var(--brd);border-radius:4px;background:var(--card);color:var(--fg);cursor:pointer;font-size:12px">⏸ 暂停</button>
</nav>
<div class="card">{{ content|safe }}</div>
</body>
</html>"#,
    ext = "html"
)]
pub struct BaseTemplate {
    pub content: String,
    pub is_active_cost: bool,
    pub is_active_requests: bool,
    pub is_active_keys: bool,
    pub is_active_traffic: bool,
    pub is_active_alerts: bool,
    pub is_active_help: bool,
}
