import 'package:flutter/material.dart';
import 'package:blaz/src/api.dart';

class MealPlanPage extends StatefulWidget {
  const MealPlanPage({super.key});
  @override
  State<MealPlanPage> createState() => _MealPlanPageState();
}

class _MealPlanPageState extends State<MealPlanPage> {
  final Map<String, List<Recipe>> _plan = {};
  DateTime _selected = _today();

  static DateTime _today() {
    final now = DateTime.now();
    return DateTime(now.year, now.month, now.day);
  }

  static String _key(DateTime d) =>
      '${d.year.toString().padLeft(4, '0')}-${d.month.toString().padLeft(2, '0')}-${d.day.toString().padLeft(2, '0')}';

  List<DateTime> _currentWeekDays() {
    final monday = _selected.subtract(Duration(days: _selected.weekday - 1));
    return List.generate(7, (i) {
      final d = monday.add(Duration(days: i));
      return DateTime(d.year, d.month, d.day);
    });
  }

  Future<void> _pickAndAssignRecipe() async {
    final recipes = await fetchRecipes();
    if (!mounted) return;

    Recipe? chosen;
    await showDialog(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('Add recipe'),
        content: DropdownButtonFormField<Recipe>(
          isExpanded: true,
          value: null,
          items: recipes
              .map((r) => DropdownMenuItem(value: r, child: Text(r.title)))
              .toList(),
          onChanged: (r) => chosen = r,
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(ctx),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(ctx),
            child: const Text('Add'),
          ),
        ],
      ),
    );

    if (chosen == null) return;
    setState(() {
      final k = _key(_selected);
      final list = _plan.putIfAbsent(k, () => <Recipe>[]);
      list.add(chosen!);
    });
  }

  @override
  Widget build(BuildContext context) {
    final week = _currentWeekDays();
    final list = _plan[_key(_selected)] ?? const <Recipe>[];

    return Column(
      children: [
        const SizedBox(height: 8),
        SingleChildScrollView(
          scrollDirection: Axis.horizontal,
          padding: const EdgeInsets.symmetric(horizontal: 8),
          child: Row(
            children: week.map((d) {
              final sel = d == _selected;
              final label = [
                'Mon',
                'Tue',
                'Wed',
                'Thu',
                'Fri',
                'Sat',
                'Sun',
              ][d.weekday - 1];
              return Padding(
                padding: const EdgeInsets.only(right: 8),
                child: ChoiceChip(
                  selected: sel,
                  label: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(label),
                      Text(
                        '${d.day}',
                        style: const TextStyle(fontWeight: FontWeight.bold),
                      ),
                    ],
                  ),
                  onSelected: (_) => setState(() => _selected = d),
                ),
              );
            }).toList(),
          ),
        ),
        const Divider(),
        Expanded(
          child: list.isEmpty
              ? const Center(child: Text('No recipes for this day'))
              : ListView.separated(
                  padding: const EdgeInsets.all(12),
                  itemCount: list.length,
                  itemBuilder: (_, i) => ListTile(
                    title: Text(list[i].title),
                    trailing: IconButton(
                      icon: const Icon(Icons.delete_outline),
                      onPressed: () => setState(() {
                        final k = _key(_selected);
                        _plan[k]!.removeAt(i);
                        if (_plan[k]!.isEmpty) _plan.remove(k);
                      }),
                    ),
                  ),
                  separatorBuilder: (_, __) => const Divider(height: 1),
                ),
        ),
        SafeArea(
          top: false,
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: Row(
              children: [
                Expanded(
                  child: FilledButton.icon(
                    onPressed: _pickAndAssignRecipe,
                    icon: const Icon(Icons.add),
                    label: const Text('Add recipe to day'),
                  ),
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }
}
