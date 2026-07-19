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
/// Sends one notification per recipe on the day before it's planned, at 20:00
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

    // Fetch reminders for next 30 days
    final reminders = await api.fetchUpcomingReminders();
    
    // Get today's date
    final now = DateTime.now();
    final today = _formatDate(now);
    
    // Group reminders by recipe to get unique recipes
    final recipesToNotify = <String, String>{};
    
    for (final r in reminders) {
      if (r.recipeTitle.isEmpty) continue;
      
      // Check if today is the day before this recipe's due date
      final dueDate = r.dueDate;
      
      // Parse dates to compare
      final todayDate = DateTime.tryParse('$today 00:00:00');
      final dueDateObj = DateTime.tryParse('$dueDate 00:00:00');
      
      if (todayDate != null && dueDateObj != null) {
        // Check if today is the day before the due date
        final dayBeforeDue = dueDateObj.subtract(const Duration(days: 1));
        final dayBeforeDueStr = _formatDate(dayBeforeDue);
        
        if (today == dayBeforeDueStr) {
          // This recipe needs notification today
          recipesToNotify[r.recipeId.toString()] = r.recipeTitle;
        }
      }
    }
    
    if (recipesToNotify.isEmpty) return;
    
    // Send one notification per recipe
    for (var i = 0; i < recipesToNotify.length; i++) {
      final recipeTitle = recipesToNotify.values.elementAt(i);
      await _showNotification(
        id: i,
        title: 'Prep Reminder: $recipeTitle',
        body: 'Prepare for $today',
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
