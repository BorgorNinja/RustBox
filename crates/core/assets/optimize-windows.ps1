# RustBox: make Windows lighter and smoother inside a virtual machine.
# Run from an elevated PowerShell (right-click optimize-windows.cmd > Run as administrator).
# Everything here can be undone from Windows Settings; nothing is deleted.
$ErrorActionPreference = 'SilentlyContinue'

Write-Host 'Turning off transparency and animations (the CPU draws every frame in a VM)...'
Set-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize' -Name EnableTransparency -Value 0 -Type DWord
Set-ItemProperty 'HKCU:\Control Panel\Desktop\WindowMetrics' -Name MinAnimate -Value '0'
Set-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -Name TaskbarAnimations -Value 0 -Type DWord
Set-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -Name ListviewAlphaSelect -Value 0 -Type DWord
Set-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced' -Name ListviewShadow -Value 0 -Type DWord
Set-ItemProperty 'HKCU:\Software\Microsoft\Windows\DWM' -Name EnableAeroPeek -Value 0 -Type DWord
Set-ItemProperty 'HKCU:\Software\Microsoft\Windows\DWM' -Name AlwaysHibernateThumbnails -Value 0 -Type DWord

Write-Host 'Switching to the High performance power plan and disabling hibernation...'
powercfg -setactive 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c | Out-Null
powercfg -h off | Out-Null

Write-Host 'Disabling background disk services (SysMain, Windows Search indexing)...'
foreach ($svc in 'SysMain', 'WSearch') {
    Stop-Service $svc -Force
    Set-Service $svc -StartupType Disabled
}

Write-Host ''
Write-Host 'Done. Sign out and back in (or restart) to apply everything.'
Write-Host 'To undo: Settings > Personalization > Colors (transparency), services.msc (SysMain, Windows Search).'
