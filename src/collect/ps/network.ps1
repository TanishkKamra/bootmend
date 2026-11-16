Try-Section 'adapters' {
  $R.adapters = @(Get-NetAdapter -Physical | ForEach-Object { $ip = Get-NetIPConfiguration -InterfaceIndex $_.ifIndex
      [ordered]@{ Name = $_.Name; Device = $_.InterfaceDescription; MAC = $_.MacAddress; Status = "$($_.Status)"; Speed = $_.LinkSpeed; MediaType = "$($_.PhysicalMediaType)"
        DriverProvider = $_.DriverProvider; DriverVersion = $_.DriverVersion; DriverDate = (D $_.DriverDate)
        IPv4 = @($ip.IPv4Address.IPAddress); Gateway = @($ip.IPv4DefaultGateway.NextHop); DNS = @($ip.DNSServer | Where-Object AddressFamily -EQ 2 | ForEach-Object { $_.ServerAddresses } | Select-Object -Unique) } })
}
Try-Section 'wifi' {
  $wi = KV (netsh wlan show interfaces); $wd = KV (netsh wlan show drivers)
  if ($wi['Description'] -or $wd['Driver']) {
    $R.wifi = [ordered]@{ Adapter = $wi['Description']; SSID = $wi['SSID']; Radio = $wi['Radio type']; Band = $wi['Band']; Channel = $wi['Channel']; Signal = $wi['Signal']
      RxMbps = $wi['Receive rate (Mbps)']; TxMbps = $wi['Transmit rate (Mbps)']; Auth = $wi['Authentication']; Cipher = $wi['Cipher']; RadiosSupported = $wd['Radio types supported']; DriverVersion = $wd['Version']; DriverDate = $wd['Date']
      WPA3 = [bool]((netsh wlan show drivers) -match 'WPA3') }
  }
}
Try-Section 'proxy' {
  $is = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Internet Settings'
  # Windows' own proxy for its services (netsh winhttp), stored as bytes: flags at offset 8 (2 = a
  # proxy is set), then the server's length and name. Read directly because netsh output is translated.
  $wh = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Internet Settings\Connections').WinHttpSettings
  $whs = $null
  if ($wh -and $wh.Length -ge 16 -and ($wh[8] -band 2)) { $n = [BitConverter]::ToInt32($wh, 12); if ($n -gt 0 -and 16 + $n -le $wh.Length) { $whs = [Text.Encoding]::ASCII.GetString($wh, 16, $n) } }
  $R.proxy = [ordered]@{ Enabled = ($is.ProxyEnable -eq 1); Server = $is.ProxyServer; Pac = $is.AutoConfigURL; WinHttp = $whs }
}
Try-Section 'staticDns' {
  # DNS servers typed in on the PC (by a person or a program) rather than handed out by the router.
  $R.staticDns = @(Get-ChildItem 'HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces' | ForEach-Object { (Get-ItemProperty $_.PSPath).NameServer } | Where-Object { $_ } | ForEach-Object { $_ -split '[ ,]+' } | Where-Object { $_ } | Select-Object -Unique)
  # The same per network adapter, IPv6 included, so each can be set back to automatic and the undo
  # restores both. Adapters that are gone are left out.
  $names = @{}; Get-NetAdapter -IncludeHidden | ForEach-Object { $names["$($_.InterfaceGuid)"] = $_.Name }
  $by = [ordered]@{}
  foreach ($ip in 'Tcpip', 'Tcpip6') {
    Get-ChildItem "HKLM:\SYSTEM\CurrentControlSet\Services\$ip\Parameters\Interfaces" | ForEach-Object { $ns = (Get-ItemProperty $_.PSPath).NameServer; $n = $names[$_.PSChildName]
      if ($ns -and $n) { $by[$n] = @($by[$n]) + @($ns -split '[ ,]+' | Where-Object { $_ }) | Where-Object { $_ } } }
  }
  $R.dnsByHand = @($by.GetEnumerator() | ForEach-Object { [ordered]@{ Adapter = $_.Key; Servers = @($_.Value) } })
}
Try-Section 'listening' {
  $R.listening = @(Get-NetTCPConnection -State Listen | Where-Object { $_.LocalAddress -notin '127.0.0.1', '::1' } | ForEach-Object {
      $pr = Get-Process -Id $_.OwningProcess; [ordered]@{ Port = [int]$_.LocalPort; Address = $_.LocalAddress; Process = $pr.ProcessName; Path = $pr.Path } } | Sort-Object { $_.Port } -Unique)
}
Try-Section 'vpn' {
  $R.virtualAdapters = @(Get-NetAdapter | Where-Object { -not $_.HardwareInterface } | ForEach-Object { "$($_.Name) ($($_.InterfaceDescription)) $($_.Status)" })
}
Emit
