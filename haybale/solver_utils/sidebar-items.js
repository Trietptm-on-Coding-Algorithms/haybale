initSidebarItems({"enum":[["PossibleSolutions",""],["SolutionCount",""]],"fn":[["bvs_can_be_equal","Returns `true` if under the current constraints, `a` and `b` can have the same value. Returns `false` if `a` and `b` cannot have the same value. (If the current constraints are themselves unsatisfiable, that will also result in `false`.)"],["bvs_must_be_equal","Returns `true` if under the current constraints, `a` and `b` must have the same value. Returns `false` if `a` and `b` may have different values. (If the current constraints are themselves unsatisfiable, that will result in `true`.)"],["get_possible_solutions_for_bv","Get a description of the possible solutions for the `BV`."],["max_possible_solution_for_bv","Get the maximum possible solution for the `BV`: that is, the highest value for which the current set of constraints is still satisfiable. \"Maximum\" will be interpreted in an unsigned fashion."],["min_possible_solution_for_bv","Get the minimum possible solution for the `BV`: that is, the lowest value for which the current set of constraints is still satisfiable. \"Maximum\" will be interpreted in an unsigned fashion."],["sat","Returns `true` if current constraints are satisfiable, `false` if not."],["sat_with_extra_constraints","Returns `true` if the current constraints plus the additional constraints `conds` are together satisfiable, or `false` if not."]]});