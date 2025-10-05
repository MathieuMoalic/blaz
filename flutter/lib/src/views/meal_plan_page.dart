import 'package:flutter/material.dart';
import 'package:intl/intl.dart';
import '../api.dart';

class MealPlanPage extends StatefulWidget {
  const MealPlanPage({super.key});
  @override
  State<MealPlanPage> createState() => _MealPlanPageState();
}

class _MealPlanPageState extends State<MealPlanPage> {
  final _fmt = DateFormat('yyyy-MM-dd');
  late String _day;
  late Future<List<MealPlanEntry>> _future;
  final _recipeIdCtrl = TextEditingController();

  @override
  void initState() {
    super.initState();
    _day = _fmt.format(DateTime.now());
    _future = fetchMealPlanForDay(_day);
  }

  @override
  void dispose() {
    _recipeIdCtrl.dispose();
    super.dispose();
  }

  Future<void> _refresh() async {
    final f = fetchMealPlanForDay(_day);
    setState(() {
      _future = f;
    });
    await f;
  }

  Future<void> _pickDay() async {
    final now = DateTime.now();
    final d = await showDatePicker(
      context: context,
      firstDate: now.subtract(const Duration(days: 365)),
      lastDate: now.add(const Duration(days: 365)),
      initialDate: now,
    );
    if (d != null) {
      _day = _fmt.format(d);
      _refresh();
    }
  }

  Future<void> _assign() async {
    final id = int.tryParse(_recipeIdCtrl.text.trim());
    if (id == null) return;
    try {
      await assignRecipeToDay(day: _day, recipeId: id);
      _recipeIdCtrl.clear();
      _refresh();
    } catch (e) {
      if (!mounted) return;
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text('Failed: $e')));
    }
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      children: [
        _HeaderRow(title: 'Meal plan – $_day', onPickDay: _pickDay),
        Padding(
          padding: const EdgeInsets.all(12),
          child: Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _recipeIdCtrl,
                  keyboardType: TextInputType.number,
                  decoration: const InputDecoration(
                    labelText: 'Recipe ID',
                    border: OutlineInputBorder(),
                  ),
                  onSubmitted: (_) => _assign(),
                ),
              ),
              const SizedBox(width: 8),
              FilledButton(onPressed: _assign, child: const Text('Assign')),
            ],
          ),
        ),
        Expanded(
          child: RefreshIndicator(
            onRefresh: _refresh,
            child: FutureBuilder<List<MealPlanEntry>>(
              future: _future,
              builder: (context, snap) {
                if (snap.connectionState != ConnectionState.done) {
                  return const Center(child: CircularProgressIndicator());
                }
                if (snap.hasError)
                  return Center(child: Text('Error: ${snap.error}'));
                final items = snap.data ?? const [];
                if (items.isEmpty)
                  return const Center(child: Text('No entries'));
                return ListView.separated(
                  itemCount: items.length,
                  padding: const EdgeInsets.all(12),
                  itemBuilder: (_, i) {
                    final m = items[i];
                    return ListTile(
                      title: Text(m.title),
                      subtitle: Text('Recipe #${m.recipeId}'),
                      trailing: IconButton(
                        icon: const Icon(Icons.delete_outline),
                        onPressed: () async {
                          await unassignRecipeFromDay(
                            day: m.day,
                            recipeId: m.recipeId,
                          );
                          _refresh();
                        },
                      ),
                    );
                  },
                  separatorBuilder: (_, __) => const Divider(height: 1),
                );
              },
            ),
          ),
        ),
      ],
    );
  }
}

class _HeaderRow extends StatelessWidget {
  final String title;
  final VoidCallback onPickDay;
  const _HeaderRow({required this.title, required this.onPickDay});
  @override
  Widget build(BuildContext context) {
    return Container(
      height: 56,
      padding: const EdgeInsets.symmetric(horizontal: 12),
      child: Row(
        children: [
          Text(title, style: Theme.of(context).textTheme.titleLarge),
          const Spacer(),
          IconButton(
            onPressed: onPickDay,
            icon: const Icon(Icons.calendar_month),
          ),
        ],
      ),
    );
  }
}
