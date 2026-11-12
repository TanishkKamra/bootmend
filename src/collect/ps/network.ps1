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
      RxMbps = $wi['Receive rate (Mbps)']; TxMbps = $wi['Transmit rate (Mbps)']; Auth = $wi['Authentication']; RadiosSupported = $wd['Radio types supported']; DriverVersion = $wd['Version']; DriverDate = $wd['Date']
      WPA3 = [bool]((netsh wlan show drivers) -match 'WPA3') }
  }
}
Try-Section 'listening' {
  $R.listening = @(Get-NetTCPConnection -State Listen | Where-Object { $_.LocalAddress -notin '127.0.0.1', '::1' } | ForEach-Object {
      $pr = Get-Process -Id $_.OwningProcess; [ordered]@{ Port = [int]$_.LocalPort; Address = $_.LocalAddress; Process = $pr.ProcessName; Path = $pr.Path } } | Sort-Object { $_.Port } -Unique)
}
Try-Section 'vpn' {
  $R.virtualAdapters = @(Get-NetAdapter | Where-Object { -not $_.HardwareInterface } | ForEach-Object { "$($_.Name) ($($_.InterfaceDescription)) $($_.Status)" })
}
Emit
