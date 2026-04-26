import 'dart:io';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter_local_notifications/flutter_local_notifications.dart';
import 'package:permission_handler/permission_handler.dart';
import 'package:workmanager/workmanager.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'api.dart' as api;

const _reminderCheckTask = 'prep-reminder-check';
const _notificationChannelId = 'prep_reminders';
const _notificationChannelName = 'Prep Reminders';

final FlutterLocalNotificationsPlugin _notifications =
    FlutterLocalNotificationsPlugin();

/// Initialize notifications and background worker (Android only)
Future<void> initNotifications() async {
  if (kIsWeb || !Platform.isAndroid) return;

  // Check if notifications are enabled in app settings
  final prefs = await SharedPreferences.getInstance();
  final enabled = prefs.getBool('notifications_enabled') ?? true;
  if (!enabled) return;

  // Request notification permission (required on Android 13+)
  final status = await Permission.notification.status;
  if (!status.isGranted) {
    final result = await Permission.notification.request();
    if (!result.isGranted) {
      return; // User denied permission
    }
  }

  // Initialize flutter_local_notifications
  const androidSettings = AndroidInitializationSettings('@mipmap/ic_launcher');
  const initSettings = InitializationSettings(android: androidSettings);
  await _notifications.initialize(initSettings);

  // Create notification channel
  const androidChannel = AndroidNotificationChannel(
    _notificationChannelId,
    _notificationChannelName,
    description: 'Notifications for upcoming meal prep reminders',
    importance: Importance.high,
  );
  await _notifications
      .resolvePlatformSpecificImplementation<
          AndroidFlutterLocalNotificationsPlugin>()
      ?.createNotificationChannel(androidChannel);

  // Initialize WorkManager for background tasks
  await Workmanager().initialize(
    _callbackDispatcher,
    isInDebugMode: false,
  );

  // Schedule periodic task (every 6 hours)
  await Workmanager().registerPeriodicTask(
    _reminderCheckTask,
    _reminderCheckTask,
    frequency: const Duration(hours: 6),
    existingWorkPolicy: ExistingPeriodicWorkPolicy.keep,
    constraints: Constraints(
      networkType: NetworkType.connected,
    ),
  );
}

/// Cancel all scheduled reminder checks
Future<void> cancelNotifications() async {
  if (kIsWeb || !Platform.isAndroid) return;
  await Workmanager().cancelByUniqueName(_reminderCheckTask);
  await _notifications.cancelAll();
}

/// Background task callback
@pragma('vm:entry-point')
void _callbackDispatcher() {
  Workmanager().executeTask((task, inputData) async {
    if (task == _reminderCheckTask) {
      await _checkAndNotifyReminders();
    }
    return Future.value(true);
  });
}

/// Check for reminders and send notifications
Future<void> _checkAndNotifyReminders() async {
  try {
    // Load stored credentials
    final prefs = await SharedPreferences.getInstance();
    final authToken = prefs.getString('auth_token');

    if (authToken == null || authToken.isEmpty) {
      return; // Not logged in
    }

    // Initialize API (loads base URL from prefs)
    await api.initApi();
    
    // Set auth token
    api.setAuthToken(authToken);

    // Fetch reminders
    final reminders = await api.fetchUpcomingReminders();
    
    // Get today and tomorrow dates
    final now = DateTime.now();
    final today = _formatDate(now);
    final tomorrow = _formatDate(now.add(const Duration(days: 1)));

    // Filter to today/tomorrow
    final urgentReminders = reminders
        .where((r) => r.dueDate == today || r.dueDate == tomorrow)
        .toList();

    if (urgentReminders.isEmpty) return;

    // Send notifications
    for (int i = 0; i < urgentReminders.length; i++) {
      final r = urgentReminders[i];
      final when = r.dueDate == today ? 'today' : 'tomorrow';
      await _showNotification(
        id: i,
        title: 'Prep Reminder: ${r.recipeTitle}',
        body: '$when: ${r.step}',
      );
    }
  } catch (e) {
    // Silently fail - background tasks shouldn't crash
  }
}

String _formatDate(DateTime date) {
  return '${date.year.toString().padLeft(4, '0')}-'
      '${date.month.toString().padLeft(2, '0')}-'
      '${date.day.toString().padLeft(2, '0')}';
}

Future<void> _showNotification({
  required int id,
  required String title,
  required String body,
}) async {
  const androidDetails = AndroidNotificationDetails(
    _notificationChannelId,
    _notificationChannelName,
    channelDescription: 'Notifications for upcoming meal prep reminders',
    importance: Importance.high,
    priority: Priority.high,
  );
  const details = NotificationDetails(android: androidDetails);
  await _notifications.show(id, title, body, details);
}

/// Send a test notification to verify the system is working
Future<bool> sendTestNotification() async {
  if (kIsWeb || !Platform.isAndroid) return false;

  try {
    // Check permission
    final status = await Permission.notification.status;
    if (!status.isGranted) {
      return false;
    }

    // Initialize if needed
    const androidSettings = AndroidInitializationSettings('@mipmap/ic_launcher');
    const initSettings = InitializationSettings(android: androidSettings);
    await _notifications.initialize(initSettings);

    await _showNotification(
      id: 999,
      title: 'Test Notification',
      body: 'Prep reminders are working!',
    );
    return true;
  } catch (e) {
    return false;
  }
}

/// Manually trigger reminder check (for testing/debugging)
Future<void> checkRemindersNow() async {
  if (kIsWeb || !Platform.isAndroid) return;
  await _checkAndNotifyReminders();
}
