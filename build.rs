/// Days since 1970-01-01 to a civil date, so the About page can say when the
/// exe was built without pulling in a date crate.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn main() {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    println!("cargo:rustc-env=WINCRAFT_BUILD_DATE={year:04}-{month:02}-{day:02}");
    println!("cargo:rerun-if-changed=src");

    // An exe that names its maker and purpose looks less like malware to
    // antivirus heuristics; Defender's ML flagged the bare 0.20.27 build.
    let mut res = winresource::WindowsResource::new();
    res.set_icon_with_id("assets/wincraft.ico", "1");
    res.set_icon_with_id("assets/wincraft-tray.ico", "2");
    res.set("ProductName", "WinCraft");
    res.set(
        "FileDescription",
        "WinCraft - Windows 11 tray utilities (open source)",
    );
    res.set("CompanyName", "WinCraft open-source project");
    res.set(
        "LegalCopyright",
        "Copyright (c) 2026 OliverD25. MIT License.",
    );
    res.set("OriginalFilename", "wincraft.exe");
    res.set("InternalName", "wincraft");
    res.set("Comments", "https://github.com/OliverD25/wincraft");
    res.set_manifest(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity type="win32" name="OliverD25.WinCraft" version="1.0.0.0" processorArchitecture="*"/>
  <description>WinCraft - Windows 11 tray utilities</description>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2</dpiAwareness>
    </windowsSettings>
  </application>
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0"
                        processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>"#,
    );
    res.compile().unwrap();
    println!("cargo:rerun-if-changed=assets/wincraft.ico");
    println!("cargo:rerun-if-changed=assets/wincraft-tray.ico");
}
