# Shared helpers, prepended to every collector script. Read-only: nothing here changes the system.
$ErrorActionPreference = 'SilentlyContinue'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$R = [ordered]@{}
$Errors = New-Object System.Collections.Generic.List[string]
function D($d) { if ($d) { try { ([datetime]$d).ToString('yyyy-MM-dd') } catch { "$d" } } else { $null } }
function GB($b, $n = 1) { if ($b) { [math]::Round($b / 1GB, $n) } else { $null } }
function Clean($s) { if ($null -eq $s) { return $null }; ("$s" -replace "`0", '').Trim() }
function KV($lines) { $h = [ordered]@{}; foreach ($l in $lines) { if ($l -match '^\s*([^:]+?)\s*:\s(.*)$') { $k = $matches[1].Trim(); if (-not $h.Contains($k)) { $h[$k] = $matches[2].Trim() } } }; $h }
function Sig($p) {
  if (-not $p -or -not (Test-Path -LiteralPath $p)) { return 'missing' }
  $s = Get-AuthenticodeSignature -LiteralPath $p
  if ($s.Status -eq 'Valid') { ($s.SignerCertificate.Subject -replace '^CN=("?)([^,"]+).*', '$2') } else { "UNSIGNED ($($s.Status))" }
}
function Try-Section($name, [scriptblock]$block) {
  try { & $block } catch { $Errors.Add("${name}: $($_.Exception.Message)") }
}
function Emit { $R.errors = @($Errors); $R | ConvertTo-Json -Depth 7 -Compress }
$IsAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
