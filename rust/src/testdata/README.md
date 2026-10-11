# Parser fixtures

`uiautomator2_AndroidManifest.xml` is a real Android binary XML manifest extracted from the
UiAutomator2 server package (`com.github.uiautomator`, version `2.4.0`). It is intentionally
**not** the target school application and must never be used for installation or live device
operations.

The fixture exists to exercise the binary XML parser against a production-shaped string pool,
integer `versionCode`, string `versionName`, package metadata, and normal manifest chunk layout.
The independent audit agent may wrap it in a minimal ZIP/APK fixture and add malformed/chained
ZIP/XML cases without touching production configuration.
