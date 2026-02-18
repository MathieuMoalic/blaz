import 'package:flutter/material.dart';
import '../api.dart' as api;

class AppStatePage extends StatefulWidget {
  const AppStatePage({super.key});

  @override
  State<AppStatePage> createState() => _AppStatePageState();
}

class _AppStatePageState extends State<AppStatePage> {
  api.LlmCredits? _credits;
  bool _loading = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    _load();
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

    return body;
  }

  Widget _stat(String label, String value) => Column(
    crossAxisAlignment: CrossAxisAlignment.start,
    children: [
      Text(label, style: const TextStyle(fontSize: 11, color: Colors.grey)),
      Text(value, style: const TextStyle(fontWeight: FontWeight.bold)),
    ],
  );
}
