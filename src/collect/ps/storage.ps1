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
