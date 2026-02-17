import 'package:flutter/material.dart';
import 'package:package_info_plus/package_info_plus.dart';
import '../api.dart' as api;

class VersionChecker extends StatefulWidget {
  final Widget child;

  const VersionChecker({super.key, required this.child});

  @override
  State<VersionChecker> createState() => _VersionCheckerState();
}

class _VersionCheckerState extends State<VersionChecker> {
  bool _hasChecked = false;
  bool _showingDialog = false;

  @override
  void initState() {
    super.initState();
    _checkVersion();
  }

  Future<void> _checkVersion() async {
    if (_hasChecked) return;
    _hasChecked = true;

    try {
      // Get app version
      final packageInfo = await PackageInfo.fromPlatform();
      final appVersion = packageInfo.version;

      // Get backend version
      final backendVersion = await api.fetchBackendVersion();

      // Compare versions (simple string comparison)
      if (appVersion != backendVersion && mounted && !_showingDialog) {
        _showingDialog = true;
        _showVersionMismatchDialog(appVersion, backendVersion);
      }
    } catch (e) {
      // Silently fail if version check fails (e.g., offline)
      debugPrint('Version check failed: $e');
    }
  }

  void _showVersionMismatchDialog(String appVersion, String backendVersion) {
    showDialog(
      context: context,
      barrierDismissible: true,
      builder: (context) => AlertDialog(
        title: const Row(
          children: [
            Icon(Icons.warning, color: Colors.orange),
            SizedBox(width: 8),
            Text('Version Mismatch'),
          ],
        ),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Text(
              'The app and backend versions do not match. This may cause compatibility issues.',
            ),
            const SizedBox(height: 16),
            Text('App version: $appVersion', style: const TextStyle(fontWeight: FontWeight.bold)),
            Text('Backend version: $backendVersion', style: const TextStyle(fontWeight: FontWeight.bold)),
            const SizedBox(height: 16),
            const Text(
              'Please update both to the same version for the best experience.',
              style: TextStyle(fontSize: 12),
            ),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () {
              Navigator.of(context).pop();
              _showingDialog = false;
            },
            child: const Text('OK'),
          ),
        ],
      ),
    ).then((_) {
      _showingDialog = false;
    });
  }

  @override
  Widget build(BuildContext context) {
    return widget.child;
  }
}
