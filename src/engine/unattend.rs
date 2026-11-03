use crate::types::WindowsTweakOptions;

/// Generates an unattended answer file (unattend.xml) matching Rufus's reverse-engineered
/// Windows User Experience engine (FUN_140062e30).
pub fn generate_unattend_xml(options: &WindowsTweakOptions) -> String {
    let mut xml = String::with_capacity(4096);
    xml.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    xml.push_str("<unattend xmlns=\"urn:schemas-microsoft-com:unattend\">\n");

    // 1. Pass: windowsPE (Bypass TPM, Secure Boot, RAM)
    if options.bypass_tpm_secureboot_ram {
        xml.push_str("  <settings pass=\"windowsPE\">\n");
        xml.push_str("    <component name=\"Microsoft-Windows-Setup\" processorArchitecture=\"amd64\" language=\"neutral\" xmlns:wcm=\"http://schemas.microsoft.com/WMIConfig/2002/State\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" publicKeyToken=\"31bf3856ad364e35\" versionScope=\"nonSxS\">\n");
        xml.push_str("      <UserData>\n");
        xml.push_str("        <AcceptEula>true</AcceptEula>\n");
        xml.push_str("        <ProductKey>\n");
        xml.push_str("          <Key />\n");
        xml.push_str("        </ProductKey>\n");
        xml.push_str("      </UserData>\n");
        xml.push_str("      <RunSynchronous>\n");
        xml.push_str("        <RunSynchronousCommand wcm:action=\"add\">\n");
        xml.push_str("          <Order>1</Order>\n");
        xml.push_str("          <Path>reg add HKLM\\SYSTEM\\Setup\\LabConfig /v BypassTPMCheck /t REG_DWORD /d 1 /f</Path>\n");
        xml.push_str("        </RunSynchronousCommand>\n");
        xml.push_str("        <RunSynchronousCommand wcm:action=\"add\">\n");
        xml.push_str("          <Order>2</Order>\n");
        xml.push_str("          <Path>reg add HKLM\\SYSTEM\\Setup\\LabConfig /v BypassSecureBootCheck /t REG_DWORD /d 1 /f</Path>\n");
        xml.push_str("        </RunSynchronousCommand>\n");
        xml.push_str("        <RunSynchronousCommand wcm:action=\"add\">\n");
        xml.push_str("          <Order>3</Order>\n");
        xml.push_str("          <Path>reg add HKLM\\SYSTEM\\Setup\\LabConfig /v BypassRAMCheck /t REG_DWORD /d 1 /f</Path>\n");
        xml.push_str("        </RunSynchronousCommand>\n");
        xml.push_str("      </RunSynchronous>\n");
        xml.push_str("    </component>\n");
        xml.push_str("  </settings>\n");
    }

    // 2. Pass: specialize (Bypass Online Account & Debloat)
    if options.bypass_online_account_nro || options.disable_telemetry_copilot {
        xml.push_str("  <settings pass=\"specialize\">\n");
        xml.push_str("    <component name=\"Microsoft-Windows-Deployment\" processorArchitecture=\"amd64\" language=\"neutral\" xmlns:wcm=\"http://schemas.microsoft.com/WMIConfig/2002/State\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" publicKeyToken=\"31bf3856ad364e35\" versionScope=\"nonSxS\">\n");
        xml.push_str("      <RunSynchronous>\n");

        let mut order = 1;

        if options.bypass_online_account_nro {
            xml.push_str(&format!(
                "        <RunSynchronousCommand wcm:action=\"add\">\n          <Order>{}</Order>\n          <Path>reg add \"HKLM\\Software\\Microsoft\\Windows\\CurrentVersion\\OOBE\" /v BypassNRO /t REG_DWORD /d 1 /f</Path>\n        </RunSynchronousCommand>\n",
                order
            ));
            order += 1;
        }

        if options.disable_telemetry_copilot {
            xml.push_str(&format!(
                "        <RunSynchronousCommand wcm:action=\"add\">\n          <Order>{}</Order>\n          <Path>reg add \"HKLM\\Software\\Policies\\Microsoft\\Windows\\WindowsCopilot\" /v TurnOffWindowsCopilot /t REG_DWORD /d 1 /f</Path>\n        </RunSynchronousCommand>\n",
                order
            ));
            order += 1;

            xml.push_str(&format!(
                "        <RunSynchronousCommand wcm:action=\"add\">\n          <Order>{}</Order>\n          <Path>reg add \"HKCU\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Search\" /v BingSearchEnabled /t REG_DWORD /d 0 /f</Path>\n        </RunSynchronousCommand>\n",
                order
            ));
            order += 1;

            xml.push_str(&format!(
                "        <RunSynchronousCommand wcm:action=\"add\">\n          <Order>{}</Order>\n          <Path>PowerShell -NonInteractive -WindowStyle Hidden -Command \"Get-AppxProvisionedPackage -Online | Where-Object {{$_.PackageName -like '*Teams*'}} | Remove-AppxProvisionedPackage -Online\"</Path>\n        </RunSynchronousCommand>\n",
                order
            ));
        }

        xml.push_str("      </RunSynchronous>\n");
        xml.push_str("    </component>\n");
        xml.push_str("  </settings>\n");
    }

    // 3. Pass: oobeSystem (OOBE screens, Local Account & BitLocker suppression)
    xml.push_str("  <settings pass=\"oobeSystem\">\n");
    xml.push_str("    <component name=\"Microsoft-Windows-Shell-Setup\" processorArchitecture=\"amd64\" language=\"neutral\" xmlns:wcm=\"http://schemas.microsoft.com/WMIConfig/2002/State\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" publicKeyToken=\"31bf3856ad364e35\" versionScope=\"nonSxS\">\n");
    xml.push_str("      <OOBE>\n");
    xml.push_str("        <HideEULAPage>true</HideEULAPage>\n");
    xml.push_str("        <ProtectYourPC>3</ProtectYourPC>\n");
    if options.bypass_online_account_nro {
        xml.push_str("        <HideOnlineAccountScreens>true</HideOnlineAccountScreens>\n");
        xml.push_str("        <HideWirelessSetupInOOBE>true</HideWirelessSetupInOOBE>\n");
    }
    xml.push_str("      </OOBE>\n");

    if options.create_local_account {
        let clean_user = sanitize_username(&options.local_account_name);
        xml.push_str("      <UserAccounts>\n");
        xml.push_str("        <LocalAccounts>\n");
        xml.push_str("          <LocalAccount wcm:action=\"add\">\n");
        xml.push_str(&format!("            <Name>{}</Name>\n", clean_user));
        xml.push_str(&format!("            <DisplayName>{}</DisplayName>\n", clean_user));
        xml.push_str("            <Group>Administrators;Power Users</Group>\n");
        xml.push_str("            <Password>\n");
        xml.push_str("              <Value>UABhAHMAcwB3AG8AcgBkAA==</Value>\n");
        xml.push_str("              <PlainText>false</PlainText>\n");
        xml.push_str("            </Password>\n");
        xml.push_str("          </LocalAccount>\n");
        xml.push_str("        </LocalAccounts>\n");
        xml.push_str("      </UserAccounts>\n");
        xml.push_str("      <FirstLogonCommands>\n");
        xml.push_str("        <SynchronousCommand wcm:action=\"add\">\n");
        xml.push_str("          <Order>1</Order>\n");
        xml.push_str(&format!("          <CommandLine>net user \"{}\" /logonpasswordchg:yes</CommandLine>\n", clean_user));
        xml.push_str("        </SynchronousCommand>\n");
        xml.push_str("        <SynchronousCommand wcm:action=\"add\">\n");
        xml.push_str("          <Order>2</Order>\n");
        xml.push_str("          <CommandLine>net accounts /maxpwage:unlimited</CommandLine>\n");
        xml.push_str("        </SynchronousCommand>\n");
        xml.push_str("      </FirstLogonCommands>\n");
    }

    xml.push_str("    </component>\n");

    if options.disable_bitlocker {
        xml.push_str("    <component name=\"Microsoft-Windows-SecureStartup-FilterDriver\" processorArchitecture=\"amd64\" language=\"neutral\" xmlns:wcm=\"http://schemas.microsoft.com/WMIConfig/2002/State\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" publicKeyToken=\"31bf3856ad364e35\" versionScope=\"nonSxS\">\n");
        xml.push_str("      <PreventDeviceEncryption>true</PreventDeviceEncryption>\n");
        xml.push_str("    </component>\n");
        xml.push_str("    <component name=\"Microsoft-Windows-EnhancedStorage-Adm\" processorArchitecture=\"amd64\" language=\"neutral\" xmlns:wcm=\"http://schemas.microsoft.com/WMIConfig/2002/State\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" publicKeyToken=\"31bf3856ad364e35\" versionScope=\"nonSxS\">\n");
        xml.push_str("      <TCGSecurityActivationDisabled>1</TCGSecurityActivationDisabled>\n");
        xml.push_str("    </component>\n");
    }

    xml.push_str("  </settings>\n");
    xml.push_str("</unattend>\n");

    xml
}

fn sanitize_username(raw: &str) -> String {
    let sanitized: String = raw.chars()
        .map(|c| if "/\\[]:;|=.,+*?<>%@&\"".contains(c) { '_' } else { c })
        .collect();
    if sanitized.trim().is_empty() {
        "User".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unattend_generation() {
        let opts = WindowsTweakOptions::default();
        let xml = generate_unattend_xml(&opts);
        assert!(xml.contains("BypassTPMCheck"));
        assert!(xml.contains("BypassSecureBootCheck"));
        assert!(xml.contains("BypassRAMCheck"));
        assert!(xml.contains("BypassNRO"));
        assert!(xml.contains("PreventDeviceEncryption"));
        assert!(xml.contains("TurnOffWindowsCopilot"));
    }
}

