import 'dart:io' show Platform;

bool get isDesktop =>
    Platform.isWindows || Platform.isLinux || Platform.isMacOS;
