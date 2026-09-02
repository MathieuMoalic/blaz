import 'package:flutter/material.dart';

/// Show a short, floating confirmation toast.
///
/// Routine success feedback: compact, floating above the bottom
/// navigation, auto-dismisses quickly. Errors should use a regular
/// [SnackBar] so they stay visible longer and can carry actions.
void showShortToast(BuildContext context, String message) {
  ScaffoldMessenger.of(context)
    ..clearSnackBars()
    ..showSnackBar(
      SnackBar(
        content: Text(message),
        behavior: SnackBarBehavior.floating,
        duration: const Duration(milliseconds: 1500),
        width: 260,
        elevation: 4,
      ),
    );
}
