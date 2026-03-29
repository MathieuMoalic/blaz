import 'dart:io';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';
import '../api.dart' as api;
import '../auth.dart';
import '../notifications.dart';
import 'login_page.dart';
import 'deleted_recipes_page.dart';

class AppStatePage extends StatefulWidget {
  const AppStatePage({super.key});

  @override
  State<AppStatePage> createState() => _AppStatePageState();
}

class _AppStatePageState extends State<AppStatePage> {
  api.LlmCredits? _credits;
  bool _loading = false;
  String? _error;
  bool _notificationsEnabled = false;
  String _selectedModel = 'anthropic/claude-3.5-sonnet';
  
  // Popular OpenRouter models for recipe parsing
  final List<Map<String, String>> _models = [
    {'id': 'anthropic/claude-3.5-sonnet', 'name': 'Claude 3.5 Sonnet'},
    {'id': 'google/gemini-2.0-flash-exp:free', 'name': 'Gemini 2.0 Flash (Free)'},
    {'id': 'openai/gpt-4o', 'name': 'GPT-4o'},
    {'id': 'deepseek/deepseek-chat', 'name': 'DeepSeek Chat'},
    {'id': 'meta-llama/llama-3.3-70b-instruct', 'name': 'Llama 3.3 70B'},
  ];

  @override
  void initState() {
    super.initState();
    _load();
    _loadNotificationSetting();
    _loadModelSetting();
  }

  Future<void> _loadNotificationSetting() async {
    if (kIsWeb || !Platform.isAndroid) return;
    final prefs = await SharedPreferences.getInstance();
    if (mounted) {
      setState(() {
        _notificationsEnabled = prefs.getBool('notifications_enabled') ?? true;
      });
    }
  }

  Future<void> _loadModelSetting() async {
    final prefs = await SharedPreferences.getInstance();
    if (mounted) {
      setState(() {
        _selectedModel = prefs.getString('llm_model') ?? 'anthropic/claude-3.5-sonnet';
      });
    }
  }

  Future<void> _setModel(String? model) async {
    if (model == null) return;
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('llm_model', model);
    
    setState(() => _selectedModel = model);
    
    if (mounted) {
      final modelName = _models.firstWhere(
        (m) => m['id'] == model, 
        orElse: () => {'name': model}
      )['name'];
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Model set to $modelName')),
      );
    }
  }

  Future<void> _toggleNotifications(bool enabled) async {
    final prefs = await SharedPreferences.getInstance();
    await prefs.setBool('notifications_enabled', enabled);
    
    if (enabled) {
      await initNotifications();
    } else {
      await cancelNotifications();
    }
    
    setState(() => _notificationsEnabled = enabled);
    
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text(
            enabled 
                ? 'Prep reminder notifications enabled' 
                : 'Prep reminder notifications disabled'
          ),
        ),
      );
    }
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final c = await api.fetchLlmCredits();
      if (mounted) setState(() => _credits = c);
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final colorScheme = Theme.of(context).colorScheme;
    final isAuthenticated = Auth.token != null;

    Widget body;
    if (_loading) {
      body = const Center(child: CircularProgressIndicator());
    } else if (_error != null) {
      body = Center(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Text(_error!, style: TextStyle(color: colorScheme.error)),
        ),
      );
    } else if (_credits != null) {
      final c = _credits!;
      final usageStr = '\$${c.usage.toStringAsFixed(4)}';
      final limitStr =
          c.limit != null ? '\$${c.limit!.toStringAsFixed(2)}' : '∞';
      final remaining = c.limit != null ? c.limit! - c.usage : null;
      final remainingStr =
          remaining != null ? '\$${remaining.toStringAsFixed(4)}' : null;

      body = Padding(
        padding: const EdgeInsets.all(16),
        child: Card(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  'OpenRouter Credits',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
                const SizedBox(height: 16),
                Row(
                  children: [
                    Expanded(child: _stat('Used', usageStr)),
                    Expanded(child: _stat('Limit', limitStr)),
                    if (remainingStr != null)
                      Expanded(child: _stat('Remaining', remainingStr)),
                  ],
                ),
                if (c.isFreeTier)
                  Padding(
                    padding: const EdgeInsets.only(top: 8),
                    child: Text(
                      'Free tier',
                      style: TextStyle(
                        fontSize: 12,
                        color: colorScheme.secondary,
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ),
      );
    } else {
      body = const SizedBox.shrink();
    }

    return Scaffold(
      appBar: AppBar(
        title: const Text('Settings'),
        automaticallyImplyLeading: false,
      ),
      body: Column(
        children: [
          body,
          if (isAuthenticated)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: Card(
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        'Recipe Import Settings',
                        style: Theme.of(context).textTheme.titleMedium,
                      ),
                      const SizedBox(height: 8),
                      DropdownButtonFormField<String>(
                        decoration: const InputDecoration(
                          labelText: 'LLM Model',
                          helperText: 'Model used for recipe parsing',
                          border: OutlineInputBorder(),
                        ),
                        value: _selectedModel,
                        items: _models.map((model) {
                          return DropdownMenuItem(
                            value: model['id'],
                            child: Text(model['name']!),
                          );
                        }).toList(),
                        onChanged: _setModel,
                      ),
                    ],
                  ),
                ),
              ),
            ),
          if (!kIsWeb && Platform.isAndroid && isAuthenticated)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: Card(
                child: SwitchListTile(
                  title: const Text('Prep reminder notifications'),
                  subtitle: const Text('Check every 6 hours for upcoming prep tasks'),
                  value: _notificationsEnabled,
                  onChanged: _toggleNotifications,
                ),
              ),
            ),
          if (isAuthenticated)
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: Card(
                child: ListTile(
                  leading: const Icon(Icons.delete_outline),
                  title: const Text('Recently Deleted'),
                  subtitle: const Text('View and restore deleted recipes'),
                  trailing: const Icon(Icons.chevron_right),
                  onTap: () {
                    Navigator.of(context).push(
                      MaterialPageRoute(
                        builder: (_) => const DeletedRecipesPage(),
                      ),
                    );
                  },
                ),
              ),
            ),
          Padding(
            padding: const EdgeInsets.all(16),
            child: isAuthenticated
                ? FilledButton.icon(
                    onPressed: () async {
                      await Auth.logout();
                      setState(() {
                        _credits = null;
                        _error = null;
                      });
                      if (mounted) {
                        ScaffoldMessenger.of(context).showSnackBar(
                          const SnackBar(content: Text('Logged out')),
                        );
                      }
                    },
                    icon: const Icon(Icons.logout),
                    label: const Text('Logout'),
                  )
                : FilledButton.icon(
                    onPressed: () async {
                      await Navigator.of(context).push(
                        MaterialPageRoute(builder: (_) => const LoginPage()),
                      );
                      setState(() => _load());
                    },
                    icon: const Icon(Icons.login),
                    label: const Text('Login'),
                  ),
          ),
        ],
      ),
    );
  }

  Widget _stat(String label, String value) => Column(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      Text(label, style: const TextStyle(fontSize: 11, color: Colors.grey)),
      Text(value, style: const TextStyle(fontWeight: FontWeight.bold)),
    ],
  );
}
