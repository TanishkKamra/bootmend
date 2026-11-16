Try-Section 'disks' {
  $R.disks = @(Get-PhysicalDisk | ForEach-Object {
      $rc = $_ | Get-StorageReliabilityCounter
      $disk = Get-Disk -Number $_.DeviceId
      [ordered]@{ Number = [int]$_.DeviceId; Model = (Clean $_.FriendlyName); Serial = (Clean $_.SerialNumber); Firmware = $_.FirmwareVersion; Bus = "$($_.BusType)"; Media = "$($_.MediaType)"
        SizeGB = (GB $_.Size 0); Health = "$($_.HealthStatus)"; OperationalStatus = "$($_.OperationalStatus)"
        TempC = $rc.Temperature; MaxTempC = $rc.TemperatureMax; WearPct = $rc.Wear; PowerOnHours = $rc.PowerOnHours; StartStopCycles = $rc.StartStopCycleCount
        ReadErrors = $rc.ReadErrorsTotal; WriteErrors = $rc.WriteErrorsTotal; ReadErrorsUncorrected = $rc.ReadErrorsUncorrected; WriteErrorsUncorrected = $rc.WriteErrorsUncorrected
        IsBoot = [bool]$disk.IsBoot; IsSystem = [bool]$disk.IsSystem; PartitionStyle = "$($disk.PartitionStyle)" } })
}
Try-Section 'volumes' {
  $bl = @{}; Get-BitLockerVolume | ForEach-Object { $bl[$_.MountPoint] = "$($_.ProtectionStatus) / $($_.VolumeStatus)" }
  $R.volumes = @(Get-Volume | Where-Object { $_.DriveType -eq 'Fixed' -and $_.Size -gt 0 } | ForEach-Object {
      $part = Get-Partition -Volume $_ | Select-Object -First 1
      [ordered]@{ Letter = $(if ($_.DriveLetter) { "$($_.DriveLetter):" } else { $null }); Label = $_.FileSystemLabel; FS = $_.FileSystem; SizeGB = (GB $_.Size); FreeGB = (GB $_.SizeRemaining)
        Health = "$($_.HealthStatus)"; BitLocker = $(if ($_.DriveLetter) { $bl["$($_.DriveLetter):"] } else { $null }); Disk = $(if ($part) { [int]$part.DiskNumber } else { $null }); PartitionType = $(if ($part) { "$($part.Type)" } else { $null }) } })
}
Try-Section 'ataSmart' {
  # Raw ATA SMART tables (admin only). Parsed in Rust; matched to disks by model name in InstanceName.
  $R.ataSmart = @(Get-CimInstance -Namespace root\wmi -ClassName MSStorageDriver_ATAPISmartData | ForEach-Object { [ordered]@{ Instance = $_.InstanceName; Data = @($_.VendorSpecific | ForEach-Object { [int]$_ }) } })
  $R.failurePredict = @(Get-CimInstance -Namespace root\wmi -ClassName MSStorageDriver_FailurePredictStatus | ForEach-Object { [ordered]@{ Instance = $_.InstanceName; PredictFailure = [bool]$_.PredictFailure; Reason = $_.Reason } })
}
Try-Section 'trim' {
  # 1 means Windows no longer tells SSDs which space is free (fsutil behavior DisableDeleteNotify).
  $R.trim = [ordered]@{ Off = ((Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem').DisableDeleteNotification -eq 1) }
}
Try-Section 'diskErrors' {
  # Storage errors and warnings in the System log over 90 days, grouped. The disk number comes from
  # "Harddisk1" or "Disk 1" in the message; Ntfs names a drive letter instead.
  $want = @{ disk = 7, 11, 15, 51, 52, 153; ntfs = 50, 55, 140; 'microsoft-windows-ntfs' = 50, 55, 140 }
  $R.diskErrors = @(Get-WinEvent -FilterHashtable @{ LogName = 'System'; Id = 7, 11, 15, 50, 51, 52, 55, 129, 140, 153; Level = 1, 2, 3; StartTime = (Get-Date).AddDays(-90) } -MaxEvents 3000 | Where-Object {
      $p = $_.ProviderName.ToLower(); ($_.Id -eq 129 -and $p -match '^(stornvme|storahci|iastor\w*|storport)$') -or ($want[$p] -contains $_.Id) } | ForEach-Object {
      $m = "$($_.Message)"
      [pscustomobject]@{ Source = $_.ProviderName; Id = $_.Id; Date = $_.TimeCreated
        Disk = $(if ($m -match '(?:Harddisk|Disk )(\d+)') { [int]$matches[1] } else { $null }); Volume = $(if ($m -match '(?:volume|VolumeId:)\s*([A-Z]):') { $matches[1].ToUpper() } else { $null }) } } |
    Group-Object Source, Id, Disk, Volume | ForEach-Object { $f = $_.Group[0]
      [ordered]@{ Source = $f.Source; Id = $f.Id; Count = $_.Count; Last = ($_.Group | Sort-Object Date -Descending | Select-Object -First 1).Date.ToString('yyyy-MM-dd'); Disk = $f.Disk; Volume = $f.Volume } })
}
Try-Section 'backup' {
  # Where Desktop, Documents and Pictures live, and whether OneDrive is signed in and running to copy them.
  $usf = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders'
  $folders = [ordered]@{ Desktop = $usf.Desktop; Documents = $usf.Personal; Pictures = $usf.'My Pictures' }
  $acc = @(Get-ChildItem 'HKCU:\Software\Microsoft\OneDrive\Accounts' | ForEach-Object { Get-ItemProperty $_.PSPath } | Where-Object { $_.UserFolder })
  $fh = Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\FileHistory'
  $R.backup = [ordered]@{
    OneDriveSignedIn = ($acc.Count -gt 0); OneDriveRunning = [bool](Get-Process OneDrive)
    FoldersInOneDrive = @($folders.Keys | Where-Object { $folders[$_] -match '\\OneDrive( - [^\\]+)?\\' })
    FileHistory = (($fh.Enabled -eq 1) -and (Test-Path "$env:LOCALAPPDATA\Microsoft\Windows\FileHistory\Configuration\Config1.xml"))
  }
}
Try-Section 'folders' {
  $R.bigFolders = @(foreach ($p in @("$env:USERPROFILE\Downloads", "$env:LOCALAPPDATA\Temp", "$env:WINDIR\Temp", "$env:WINDIR\SoftwareDistribution\Download", "$env:LOCALAPPDATA\CrashDumps")) {
      if (Test-Path $p) { $s = (Get-ChildItem $p -Recurse -Force -File | Measure-Object Length -Sum).Sum; [ordered]@{ Path = $p; SizeGB = [math]::Round([double]$s / 1GB, 2) } } })
}
Emit
