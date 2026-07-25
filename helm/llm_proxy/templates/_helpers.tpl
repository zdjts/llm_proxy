{{- define "llm-proxy.fullname" -}}
{{- printf "%s-%s" .Release.Name .Chart.Name | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "llm-proxy.labels" -}}
app.kubernetes.io/name: {{ include "llm-proxy.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{- define "llm-proxy.selectorLabels" -}}
app.kubernetes.io/name: {{ include "llm-proxy.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{- define "llm-proxy.name" -}}
{{- .Chart.Name | trunc 63 | trimSuffix "-" }}
{{- end }}

{{- define "llm-proxy.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "llm-proxy.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}
