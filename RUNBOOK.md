# llm_proxy RUNBOOK

## H. Admin Dashboard 访问指南 (ADR-015 §7)

### 路由总览

| 路由 | 屏幕 | 说明 |
|------|------|------|
| `/admin` | 成本总览 | 24h stat cards + per-model 成本表, `?tenant=` 切片, `?format=csv` |
| `/admin/requests` | 请求明细 | 最近 24h request_log 行, 7 个过滤器, tenant/format 支持 |
| `/admin/keys` | Key 健康 | 每 pool 的 key 状态 + 7d 成功率 sparkline (不切 tenant) |
| `/admin/traffic` | 流量趋势 | SVG 折线图 (请求量/延迟), `?days=`, `?tenant=` |
| `/admin/alerts` | 告警事件 | alert_event 表最近 100 条, 4 字段筛选 |
| `/admin/cost/drilldown` | 成本下钻 | `?model=&tenant=`, 3 SVG 折线 + 3 统计卡 |
| `/admin/help` | 帮助 | 本 RUNBOOK 全文 |

### 访问控制 (IP Guard)

admin 路由受 `config.admin.allowed_ips` 白名单保护，默认 `["127.0.0.1", "::1"]`。
前端 proxy 需设置 `X-Real-IP` 或 `X-Forwarded-For` 头。
未在白名单的请求返回 404 (不暴露路由存在)。

### Tenant 切片

cost/requests/traffic/drilldown 四屏支持 `?tenant=<tenant_id>` 查询参数。
tenant 下拉框动态填充自 `SELECT DISTINCT tenant_id FROM request_log`。
不传 tenant 参数 = 展示全部 tenant 汇总。

### CSV 导出

cost/requests/traffic 三屏支持 `?format=csv` — 返回 `Content-Type: text/csv` 含 `Content-Disposition: attachment`。

### 告警通道

Webhook 签名: 空 `webhook_secret` = 不加 `X-LLM-Proxy-Signature`。
Slack/Discord/Email 通道通过 `alerts.channels.*` config 控制。Email 为 stub (v0.8 实装)。

## X. Webhook HMAC 接收端校验 (ADR-013 §4)

网关每个告警 POST 会附带 `X-LLM-Proxy-Signature` 头，格式为：

```
X-LLM-Proxy-Signature: t=<unix_seconds>,v1=<hex_hmac_sha256>
```

### 签名构造

```
payload = b"<t>." + http_body_bytes
mac = HMAC-SHA256(webhook_secret, payload)
v1 = hex(mac)
```

### 校验 (5 分钟重放窗口)

#### Node.js

```js
const crypto = require('crypto');

function verify(reqBody, sigHeader, secret) {
  const parts = Object.fromEntries(
    sigHeader.split(',').map(p => p.split('=', 2))
  );
  const t = parseInt(parts.t, 10);
  if (Math.abs(Date.now() / 1000 - t) > 300) {
    throw new Error('stale signature');
  }
  const payload = `${t}.${reqBody}`;
  const expected = crypto
    .createHmac('sha256', secret)
    .update(payload)
    .digest('hex');
  if (!crypto.timingSafeEqual(Buffer.from(expected), Buffer.from(parts.v1))) {
    throw new Error('invalid signature');
  }
}
```

#### Python

```python
import hmac, hashlib, time

def verify(body: bytes, sig_header: str, secret: str) -> None:
    parts = dict(p.split('=', 1) for p in sig_header.split(','))
    t = int(parts['t'])
    if abs(time.time() - t) > 300:
        raise ValueError('stale signature')
    msg = f"{t}.".encode() + body
    expected = hmac.new(secret.encode(), msg, hashlib.sha256).hexdigest()
    if not hmac.compare_digest(expected, parts['v1']):
        raise ValueError('invalid signature')
```
