import 'dart:io';
import 'package:flutter/foundation.dart' show kIsWeb;
import 'package:flutter/material.dart';
import 'package:package_info_plus/package_info_plus.dart';
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
  String? _appVersion;
  String? _backendVersion;
  
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
    _loadVersions();
  }

  Future<void> _loadVersions() async {
    try {
      final packageInfo = await PackageInfo.fromPlatform();
      final backendVer = await api.fetchBackendVersion();
      if (mounted) {
        setState(() {
          _appVersion = packageInfo.version;
          _backendVersion = backendVer;
        });
      }
    } catch (e) {
      // Silently fail - versions are optional info
      if (mounted) {
        setState(() {
          _appVersion = 'Unknown';
          _backendVersion = 'Unknown';
        });
      }
    }
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

    return Scaffold(
      appBar: AppBar(
        title: const Text('Settings'),
        automaticallyImplyLeading: false,
      ),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          if (_loading)
            const Center(child: CircularProgressIndicator())
          else if (_error != null)
            Center(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Text(_error!, style: TextStyle(color: colorScheme.error)),
              ),
            )
          else if (_credits != null) ...[
            Card(
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
                        Expanded(child: _stat('Used', '\$${_credits!.usage.toStringAsFixed(4)}')),
                        Expanded(child: _stat('Limit', _credits!.limit != null ? '\$${_credits!.limit!.toStringAsFixed(2)}' : '∞')),
                        if (_credits!.limit != null)
                          Expanded(child: _stat('Remaining', '\$${(_credits!.limit! - _credits!.usage).toStringAsFixed(4)}')),
                      ],
                    ),
                    if (_credits!.isFreeTier)
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
            const SizedBox(height: 12),
          ],
          if (isAuthenticated) ...[
            Card(
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
            const SizedBox(height: 12),
          ],
          if (!kIsWeb && Platform.isAndroid && isAuthenticated) ...[
            Card(
              child: SwitchListTile(
                title: const Text('Prep reminder notifications'),
                subtitle: const Text('Check every 6 hours for upcoming prep tasks'),
                value: _notificationsEnabled,
                onChanged: _toggleNotifications,
              ),
            ),
            const SizedBox(height: 12),
          ],
          if (isAuthenticated) ...[
            Card(
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
            const SizedBox(height: 12),
          ],
          if (_appVersion != null || _backendVersion != null) ...[
            Card(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Version',
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    const SizedBox(height: 12),
                    if (_appVersion != null)
                      Row(
                        children: [
                          const Icon(Icons.phone_android, size: 16),
                          const SizedBox(width: 8),
                          Text('App: $_appVersion', style: const TextStyle(fontSize: 13)),
                        ],
                      ),
                    if (_appVersion != null && _backendVersion != null)
                      const SizedBox(height: 8),
                    if (_backendVersion != null)
                      Row(
                        children: [
                          const Icon(Icons.dns, size: 16),
                          const SizedBox(width: 8),
                          Text('Server: $_backendVersion', style: const TextStyle(fontSize: 13)),
                        ],
                      ),
                  ],
                ),
              ),
            ),
            const SizedBox(height: 12),
          ],
          Card(
            child: ListTile(
              leading: Icon(isAuthenticated ? Icons.logout : Icons.login),
              title: Text(isAuthenticated ? 'Logout' : 'Login'),
              onTap: () async {
                if (isAuthenticated) {
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
                } else {
                  await Navigator.of(context).push(
                    MaterialPageRoute(builder: (_) => const LoginPage()),
                  );
                  setState(() => _load());
                }
              },
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
