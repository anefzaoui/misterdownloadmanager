# Summarises a labserver JSONL log: one row per request, plus connection/ramp statistics.
param([Parameter(Mandatory)][string]$Log)
$e = Get-Content $Log | ForEach-Object { $_ | ConvertFrom-Json } | Sort-Object t_start_ms
$t0 = ($e | Select-Object -First 1).t_start_ms
"requests: $($e.Count)   distinct TCP connections: $(($e.peer | Sort-Object -Unique).Count)"
"user agents: " + (($e.user_agent | Sort-Object -Unique) -join " | ")
$e | Format-Table req,
  @{n='port';e={$_.peer.Split(':')[-1]}}, conn_req,
  @{n='t0+ms';e={$_.t_start_ms - $t0}}, @{n='dur ms';e={$_.t_end_ms - $_.t_start_ms}},
  method, range, if_range, status,
  @{n='start MB';e={[math]::Round($_.body_start/1MB,3)}},
  @{n='len MB';e={[math]::Round($_.body_len/1MB,3)}},
  @{n='sent MB';e={[math]::Round($_.bytes_sent/1MB,3)}}, outcome -AutoSize | Out-String -Width 300
