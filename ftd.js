// all ftd_utils are meant to be pure functions only: they can only depend on the
// input passed, not on closures or global data etc

function console_log(...message) {
    if (true) { // false
        console.log(...message);
    }
}


let ftd_utils = {
    resolve_reference: function (value, reference, data, obj) {
        let data_value_ref = (reference !== null && !(value instanceof Object)) ?
            ftd_utils.get_data_with_default(data, reference) : null;
        if (value instanceof Object) {
            let result = value instanceof Array ? [] : {};
            for (var key of Object.keys(value)) {
                if (((typeof value[key]) === "object") && (reference[key] !== undefined)) {
                    result[key] = ftd_utils.resolve_reference(value[key], reference[key], data);
                } else if (reference[key] !== undefined && reference[key] !== null) {
                    let data_value = ftd_utils.get_data_value(data, reference[key])
                    result[key] = (data_value !== undefined) ? data_value : value[key];
                } else {
                    result[key] = (value[key] === "$VALUE" && obj.value !== undefined) ? obj.value : value[key];
                }
            }
            for (var key of Object.keys(reference)) {
                let data_value = ftd_utils.get_data_value(data, reference[key])
                if (value[key] === undefined && data_value !== undefined) {
                    result[key] = data_value;
                }
            }
            return result;
        } else if (reference !== null && reference !== undefined && data_value_ref !== undefined) {
            return data_value_ref;
        } else {
            return (value === "$VALUE" && obj.value !== undefined) ? obj.value : value;
        }
    },

    is_visible: function (id, affected_id) {
        let node = document.querySelector(`[data-id="${affected_id}:${id}"]`);
        while (!!node) {
            if (!node.style) {
                return true;
            }
            if (node.style.display === "none") {
                return false;
            }
            node = node.parentNode;
        }
        return true;
    },

    box_shadow_value_null: function (value) {
        return (value === "0px 0px 0px 0px") ? null : value;
    },

    box_shadow_value: function (parameter, data_id, value) {
        let current_value  = document.querySelector(`[data-id="${data_id}"]`).style.getPropertyValue('box-shadow');
        if (current_value.length === 0) {
            current_value = "0px 0px 0px 0px";
        }
        let first_split = current_value.split(') ');
        if (first_split.length === 1) {
            first_split.unshift('');
        } else {
            first_split[0] = `${first_split[0]})`;
        }
        if (parameter === "shadow-color") {
            if (value === null) {
                return ftd_utils.box_shadow_value_null(first_split[1].trim());
            }
            first_split[0] = value;
            return ftd_utils.box_shadow_value_null(first_split.join(' ').trim());
        }
        let second_split =  first_split[1].split(' ');
        if (parameter === "shadow-offset-x") {
            second_split[0] = value !== null ? value : '0px' ;
            first_split[1] = second_split.join(' ');
            return ftd_utils.box_shadow_value_null(first_split.join(' ').trim());
        }
        if (parameter === "shadow-offset-y") {
            second_split[1] = value !== null ? value : '0px' ;
            first_split[1] = second_split.join(' ');
            return ftd_utils.box_shadow_value_null(first_split.join(' ').trim());
        }
        if (parameter === "shadow-blur") {
            second_split[2] = value !== null ? value : '0px' ;
            first_split[1] = second_split.join(' ');
            return ftd_utils.box_shadow_value_null(first_split.join(' ').trim());
        }
        if (parameter === "shadow-size") {
            second_split[3] = value !== null ? value : '0px' ;
            first_split[1] = second_split.join(' ');
            return ftd_utils.box_shadow_value_null(first_split.join(' ').trim());
        }
    },

    align_value: function (data_id, value) {
        let current_position  = document.querySelector(`[data-id="${data_id}"]`).style.getPropertyValue('position');
        if (current_position === "fixed" || current_position === "absolute") {
            document.querySelector(`[data-id="${data_id}"]`).style.setProperty('left', null);
            document.querySelector(`[data-id="${data_id}"]`).style.setProperty('top', null);
            document.querySelector(`[data-id="${data_id}"]`).style.setProperty('transform', null);
            document.querySelector(`[data-id="${data_id}"]`).style.setProperty('right', null);
            document.querySelector(`[data-id="${data_id}"]`).style.setProperty('bottom', null);
            if (value === "center") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('left', '50%');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('top', '50%');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('transform', 'translate(-50%,-50%)');
            } else if (value === "top") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('left', '50%');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('top', '0');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('transform', 'translateX(-50%)');
            } else if (value === "left") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('left', '0');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('top', '50%');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('transform', 'translateY(-50%)');
            } else if (value === "right") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('right', '0');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('top', '50%');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('transform', 'translate(-50%)');
            } else if (value === "bottom") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('left', '50%');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('bottom', '0');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('transform', 'translateX(-50%)');
            } else if (value === "top-left") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('left', '0');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('top', '0');
            } else if (value === "top-right") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('right', '0');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('top', '0');
            } else if (value === "bottom-left") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('left', '0');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('bottom', '0');
            } else if (value === "bottom-right") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('right', '0');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('bottom', '0');
            }
        } else {
            document.querySelector(`[data-id="${data_id}"]`).style.setProperty('align-self', null);
            document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-bottom',null);
            document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-top',null);
            document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-left',null);
            if (value === "center") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('align-self', 'center');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-bottom', 'auto');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-top', 'auto');
            } else if (value === "top") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('align-self', 'center');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-bottom', 'auto');
            } else if (value === "left") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('align-self', 'flex-start');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-bottom', 'auto');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-top', 'auto');
            } else if (value === "right") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('align-self', 'flex-end');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-bottom', 'auto');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-top', 'auto');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-left', 'auto');
            } else if (value === "bottom") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('align-self', 'center');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-bottom', '0');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-top', 'auto');
            } else if (value === "top-left") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('align-self', 'flex-start');
            } else if (value === "top-right") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('align-self', 'flex-end');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-left', 'auto');
            } else if (value === "bottom-left") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('align-self', 'flex-start');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-bottom', '0');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-top', 'auto');
            } else if (value === "bottom-right") {
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('align-self', 'flex-end');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-bottom', '0');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-top', 'auto');
                document.querySelector(`[data-id="${data_id}"]`).style.setProperty('margin-left', 'auto');
            }
        }

    },

    line_clamp: function (data_id, value) {
        let doc = document.querySelector(`[data-id="${data_id}"]`);
        if (value == null) {
            doc.style.setProperty('display', null);
            doc.style.setProperty('overflow', null);
            doc.style.setProperty('-webkit-line-clamp', null);
            doc.style.setProperty('-webkit-box-orient', null);
        } else {
            doc.style.setProperty('display', '-webkit-box');
            doc.style.setProperty('overflow', 'hidden');
            doc.style.setProperty('-webkit-line-clamp', value);
            doc.style.setProperty('-webkit-box-orient', 'vertical');
        }
    },

    background_image: function (data_id, value) {
        let background_repeat = document.querySelector(`[data-id="${data_id}"]`).style.getPropertyValue('background-repeat');
        let doc = document.querySelector(`[data-id="${data_id}"]`);
        if (value == null) {
            doc.style.setProperty('background-image', null);
            doc.style.setProperty('background-size', null);
            doc.style.setProperty('background-position', null);
        } else {
            doc.style.setProperty('background-image', `url(${value})`);
            if (background_repeat.length === 0) {
                doc.style.setProperty('background-size', 'cover');
                doc.style.setProperty('background-position', 'center');
            }
        }
    },

    background_repeat: function (data_id, value) {
        let doc = document.querySelector(`[data-id="${data_id}"]`);
        if (value == null) {
            doc.style.setProperty('background-repeat', null);
            doc.style.setProperty('background-size', 'cover');
            doc.style.setProperty('background-position', 'center');
        } else {
            doc.style.setProperty('background-repeat', 'repeat');
            doc.style.setProperty('background-size', null);
            doc.style.setProperty('background-position', null);
        }
    },

    first_child_styling: function (data_id) {
        let parent = document.querySelector(`[data-id="${data_id}"]`).parentElement;
        if (parent.dataset.spacing !== undefined) {
            let spacing = parent.dataset.spacing.split(":");
            let property = spacing[0].trim();
            let value = spacing[1].trim();
            let first_child = true;
            for (let i = 0; i < parent.children.length; i++) {
                if (!first_child) {
                    parent.children[i].style.setProperty(property, value);
                } else if (parent.children[i].style.display !== 'none') {
                    parent.children[i].style.setProperty(property, null);
                    first_child = false;
                }
            }
        }
    },


    set_style: function (parameter, data_id, value, important) {
        if (["shadow-offset-x", "shadow-offset-y", "shadow-size", "shadow-blur", "shadow-color"].includes(parameter)) {
            let box_shadow_value = ftd_utils.box_shadow_value(parameter,data_id, value);
            document.querySelector(`[data-id="${data_id}"]`).style.setProperty('box-shadow', box_shadow_value);
        } else if (parameter === "align" || parameter === "position") {
            ftd_utils.align_value(data_id, value);
        } else if (parameter === "line-clamp") {
            ftd_utils.line_clamp(data_id, value);
        } else if (parameter === "background-image") {
            ftd_utils.background_image(data_id, value);
        } else if (parameter === "background-repeat") {
            ftd_utils.background_repeat(data_id, value);
        } else if (important) {
            document.querySelector(`[data-id="${data_id}"]`).style.setProperty(`${parameter}`, value, 'important');
        } else {
            document.querySelector(`[data-id="${data_id}"]`).style.setProperty(`${parameter}`, value);
        }
    },

    isJson: function(str) {
        try {
            JSON.parse(str);
        } catch (e) {
            return false;
        }
        return true;
    },

    getString: (function() {
        var DIV = document.createElement("div");

        if ('outerHTML' in DIV)
            return function(node) {
                return node.outerHTML;
            };

        return function(node) {
            var div = DIV.cloneNode();
            div.appendChild(node.cloneNode(true));
            return div.innerHTML;
        };

    })(),

    create_dom: function (value,  node) {
        let dom_ids = [];
        let parent_node = node.parentElement;
        if (value instanceof Object) {
            for (const idx in value) {
                var new_node = node.cloneNode(true);
                new_node.style.display = null;
                let id = new_node.getAttribute("data-id").replace(":dummy", ",".concat(idx, ":new"));
                new_node.setAttribute("data-id", id);
                dom_ids.push(id);
                parent_node.innerHTML += ftd_utils.getString(new_node).replace("$loop$", value[idx]);
            }
        } else {
            var new_node = node.cloneNode(true);
            new_node.style.display = null;
            let id = new_node.getAttribute("data-id").replace(":dummy", ",0:new");
            new_node.setAttribute("data-id", id);
            dom_ids.push(id);
            parent_node.innerHTML += ftd_utils.getString(new_node).replace("$loop$", value);
        }
        return dom_ids;
    },

    remove_nodes: function (nodes, id) {
        for (const node in nodes) {
            console_log(`${nodes[node]}:${id}`);
            document.querySelector(`[data-id="${nodes[node]}:${id}"]`).remove();
        }
    },

    is_equal_condition: function (value, condition) {
        let val = value === null ? "" : value.toString().replaceAll("\"", "");
        return ((value === condition)
            || (condition === "$IsNull$" && (val.trim().length === 0 || value === null))
            || (condition === "$IsNotNull$" && (val.trim().length !== 0 && value !== null))
        );
    },

    get_name_and_remaining: function(name) {
        let part1 = "";
        let pattern_to_split_at = name;
        let parent_split = ftd_utils.split_once(name, "#");
        if (parent_split.length === 2) {
            part1 = parent_split[0] + "#";
            pattern_to_split_at = parent_split[1];
        }
        parent_split = ftd_utils.split_once(pattern_to_split_at, ".");
        if (parent_split.length === 2) {
            return [part1 + parent_split[0], parent_split[1]];
        }
        return [name, null];
    },

    split_once: function (name, split_at) {
        const i = name.indexOf(split_at);
        if (i === -1) {
            return [name];
        }
        return [name.slice(0, i), name.slice(i + 1)];
    },

    deepEqual: function (object1, object2) {
        const areObjects = ftd_utils.isObject(object1) && ftd_utils.isObject(object2);
        if (!areObjects && object1 !== object2) {
            return false;
        }
        if (!areObjects) {
            return true;
        }
        const keys1 = Object.keys(object1);
        const keys2 = Object.keys(object2);
        if (keys1.length !== keys2.length) {
            return false;
        }
        for (const key of keys1) {
            const val1 = object1[key];
            const val2 = object2[key];
            if (!ftd_utils.deepEqual(val1, val2)) {
                return false;
            }
        }
        return true;
    },

    isObject: function (object) {
        return object != null && typeof object === 'object';
    },

    deepCopy: function (object) {
        if (ftd_utils.isObject(object)) {
            return JSON.parse(JSON.stringify(object));
        }
        return object;
    },

    get_data_value: function (data, name) {
        let [var_name, remaining] = ftd_utils.get_name_and_remaining(name);
        let initial_value = data[var_name].value;
        while (!!remaining) {
            let [p1, p2] = ftd_utils.split_once(remaining, ".");
            if (ftd_utils.isJson(initial_value)) {
                initial_value = JSON.parse(initial_value)[p1]
            } else {
                initial_value = initial_value[p1];
            }
            remaining = p2;
        }
        return ftd_utils.deepCopy(initial_value);
    },

    get_data_with_default: function (data, name, def) {
        try {
            return ftd_utils.get_data_value(data, name);
        } catch (e) {
            return def;
        }
    },

    set_data_value: function (data, name, value) {
        let [var_name, remaining] = ftd_utils.get_name_and_remaining(name);
        let initial_value = data[var_name].value;
        if (ftd_utils.isJson(initial_value)) {
            initial_value = JSON.parse(initial_value);
        }
        data[var_name].value = ftd_utils.deepCopy(set(initial_value, remaining, value));

        function set(initial_value, remaining, value) {
            if (!remaining) {
                return value;
            }
            let [p1, p2] = ftd_utils.split_once(remaining, ".");
            initial_value[p1] = set(initial_value[p1], p2, value);
            return initial_value;
        }
    },

    handle_action: function (id, target_variable, value, data, ftd_external_children) {
        var styles_edited = [];
        let visibility_change = false;
        let current_value = ftd_utils.get_data_value(data, target_variable);
        if (current_value === value) {
            return;
        }
        handle_action_(id, target_variable, value, data, ftd_external_children, styles_edited, visibility_change);

        function handle_action_(id, target_variable, value, data, ftd_external_children, styles_edited, visibility_change) {
            ftd_utils.set_data_value(data, target_variable, value);
            let full_value = ftd_utils.get_data_value(data, target_variable);
            let new_value = full_value;
            if (!!new_value && !!new_value["$kind$"]) {
                new_value = new_value[new_value["$kind$"]];
            }
            let [target, target_remaining] = ftd_utils.get_name_and_remaining(target_variable);

            let dependencies = data[target].dependencies;
            for (const dependency in dependencies) {
                if (!dependencies.hasOwnProperty(dependency)) {
                    continue;
                }
                let json_dependencies = dependencies[dependency];
                for (const index in json_dependencies) {
                    let json_dependency = json_dependencies[index];
                    if (json_dependency.dependency_type === "Value") {
                        if (dependency.endsWith(':dummy')) {
                            let dummy_node = document.querySelector(`[data-id="${dependency}:${id}"]`);
                            let dom_ids = ftd_utils.create_dom(full_value, dummy_node);
                            ftd_utils.remove_nodes(Object.keys(dependencies).filter(s => !s.endsWith(':dummy')), id);
                            let deps = {};
                            for (const dom_id in dom_ids) {
                                let id_without_main = dom_ids[dom_id].substring(0, dom_ids[dom_id].length - `:${id}`.length)
                                deps[id_without_main] = dependencies[dependency];
                            }
                            deps[dependency] = dependencies[dependency];
                            data[target].dependencies = deps;
                        } else {
                            if ((!!json_dependency.remaining)
                                && (json_dependency.remaining !== target_remaining)) {
                                continue;
                            }
                            let doc = document.querySelector(`[data-id="${dependency}:${id}"]`);
                            if (doc.nodeName === "INPUT") {
                                doc.value = new_value;
                            } else if (doc.src !== undefined) {
                                doc.src = new_value;
                            } else {
                                doc.innerText = new_value;
                            }
                        }
                    } else if (json_dependency.dependency_type === "Visible") {
                        let display = "none";
                        if (ftd_utils.is_equal_condition(full_value, json_dependency.condition)) {
                            let is_flex = !!document.querySelector(`[data-id="${dependency}:${id}"]`).style.flexDirection.length;
                            let is_grid = !!document.querySelector(`[data-id="${dependency}:${id}"]`).style.gridTemplateAreas.length;
                            let is_webkit = !!document.querySelector(`[data-id="${dependency}:${id}"]`).style.webkitLineClamp.length;
                            if (is_flex) {
                                display = "flex";
                            } else if (is_webkit) {
                                display = "-webkit-box";
                            } else if (is_grid) {
                                display = "grid";
                            } else {
                                display = "block";
                            }
                        }
                        let node = document.querySelector(`[data-id="${dependency}:${id}"]`);
                        if (node.style.display !== display) {
                            visibility_change = true;
                        }
                        document.querySelector(`[data-id="${dependency}:${id}"]`).style.display = display;
                        ftd_utils.first_child_styling(`${dependency}:${id}`);

                    } else if (json_dependency.dependency_type === "Variable") {
                        if (json_dependency.condition === null) {
                            if (dependency === "$style$") {
                                for (const parameter in json_dependency.parameters) {
                                    let param_val = json_dependency.parameters[parameter].value.value;
                                    let node = param_val["$node$"];
                                    let var_ = param_val["$variable$"];
                                    let dependent = ftd_utils.get_data_value(data, var_);
                                    let variable = ftd_utils.get_name_and_remaining(var_)[0];
                                    let dependent_dependencies = data[variable].dependencies[node];
                                    let call = false;
                                    for (const d in dependent_dependencies) {
                                        if (dependent_dependencies[d].dependency_type !== "Style"
                                            || !dependent_dependencies[d].parameters[parameter]) {
                                            continue;
                                        }
                                        if (dependent_dependencies[d].parameters[parameter].value.reference === target) {
                                            if (ftd_utils.is_equal_condition(dependent, dependent_dependencies[d].condition)
                                                && !ftd_utils.deepEqual(dependent_dependencies[d].parameters[parameter].value.value, full_value)) {
                                                call = true;
                                            }
                                            dependent_dependencies[d].parameters[parameter].value.value = full_value;
                                        }
                                        if (!!dependent_dependencies[d].parameters[parameter].default && dependent_dependencies[d].parameters[parameter].default.reference === target) {
                                            if (ftd_utils.is_equal_condition(dependent, dependent_dependencies[d].condition)
                                                && !ftd_utils.deepEqual(dependent_dependencies[d].parameters[parameter].default.value, full_value)) {
                                                call = true;
                                            }
                                            dependent_dependencies[d].parameters[parameter].default.value = full_value;
                                        }
                                    }
                                    data[variable].dependencies[node] = dependent_dependencies;
                                    if (call) {
                                        handle_action_(id, variable, dependent, data, ftd_external_children, styles_edited, visibility_change);
                                    }
                                }
                            }
                        } else if (ftd_utils.is_equal_condition(full_value, json_dependency.condition)) {
                            for (const parameter in json_dependency.parameters) {
                                if (data[ftd_utils.get_name_and_remaining(parameter)[0]] === undefined) {
                                    continue;
                                }
                                let value = json_dependency.parameters[parameter].value.value;
                                let call = false;
                                if (dependency === "$value#kind$") {
                                    let get_value = ftd_utils.get_data_value(data, parameter + ".$kind$");
                                    if (!ftd_utils.deepEqual(value, get_value)) {
                                        call = true;
                                    }
                                    ftd_utils.set_data_value(data, parameter + ".$kind$", value);
                                }
                                if (dependency === "$value$") {
                                    let get_value = ftd_utils.get_data_value(data, parameter);
                                    if (!ftd_utils.deepEqual(value, get_value)) {
                                        call = true;
                                    }
                                    ftd_utils.set_data_value(data, parameter, value);
                                }
                                let parameter_value = ftd_utils.get_data_value(data, parameter);
                                if (call) {
                                    handle_action_(id, parameter, parameter_value, data, ftd_external_children, styles_edited, visibility_change);
                                }
                            }
                        } else {
                            for (const parameter in json_dependency.parameters) {
                                if (data[ftd_utils.get_name_and_remaining(parameter)[0]] === undefined) {
                                    continue;
                                }
                                let dep_default = json_dependency.parameters[parameter].default;
                                let call = false;
                                if (dep_default === null) {
                                    continue;
                                }
                                let value = dep_default.value;
                                if (dependency === "$value#kind$") {
                                    let get_value = ftd_utils.get_data_value(data, parameter + ".$kind$");
                                    if (!ftd_utils.deepEqual(value, get_value)) {
                                        call = true;
                                    }
                                    ftd_utils.set_data_value(data, parameter + ".$kind$", value);
                                }
                                if (dependency === "$value$") {
                                    let get_value = ftd_utils.get_data_value(data, parameter);
                                    if (!ftd_utils.deepEqual(value, get_value)) {
                                        call = true;
                                    }
                                    ftd_utils.set_data_value(data, parameter, value);
                                }
                                let parameter_value = ftd_utils.get_data_value(data, parameter);
                                if (call) {
                                    handle_action_(id, parameter, parameter_value, data, ftd_external_children, styles_edited, visibility_change);
                                }
                            }
                        }
                    } else if (json_dependency.dependency_type === "Style") {
                        if (json_dependency.condition === null || json_dependency.condition === undefined) {
                            let set = [];
                            if (!!json_dependency.parameters["dependents"]) {
                                set = json_dependency.parameters["dependents"].value.value;
                            }
                            if ((!!json_dependency.remaining)
                                && (json_dependency.remaining !== target_remaining)) {
                                continue;
                            }
                            if (!!set.length) {
                                let style_attr = Object.keys(json_dependency.parameters).filter(w => w !== "dependents")[0];
                                let call = false;
                                for (const idx in set) {
                                    let dependent = ftd_utils.get_data_value(data, set[idx]);
                                    let variable = ftd_utils.get_name_and_remaining(set[idx])[0];
                                    let dependent_dependencies = data[variable].dependencies[dependency];
                                    for (const d in dependent_dependencies) {
                                        if (dependent_dependencies[d].dependency_type !== "Style"
                                            || !dependent_dependencies[d].parameters[style_attr]) {
                                            continue;
                                        }
                                        let current_value = dependent_dependencies[d].parameters[style_attr].default.value;
                                        if (!ftd_utils.deepEqual(current_value, full_value)) {
                                            call = true;
                                        }
                                        dependent_dependencies[d].parameters[style_attr].default.value = full_value;
                                    }
                                    data[variable].dependencies[dependency] = dependent_dependencies;
                                    if (call) {
                                        handle_action_(id, set[idx], dependent, data, ftd_external_children, styles_edited, visibility_change);
                                    }
                                }
                                continue;
                            }
                            for (const parameter in json_dependency.parameters) {
                                if (parameter === "dependents") {
                                    continue;
                                }
                                let important = json_dependency.parameters[parameter].value.important;
                                if (new_value instanceof Object) {
                                    for (const parameter in new_value) {
                                        ftd_utils.set_style(parameter, `${dependency}:${id}`, new_value[parameter], important);
                                        if (!styles_edited.includes(`${parameter}::${dependency}`)) {
                                            styles_edited.push(`${parameter}::${dependency}`);
                                        }
                                    }
                                } else {
                                    ftd_utils.set_style(parameter, `${dependency}:${id}`, new_value, important);
                                    if (!styles_edited.includes(`${parameter}::${dependency}`)) {
                                        styles_edited.push(`${parameter}::${dependency}`);
                                    }
                                }
                            }
                        } else if (ftd_utils.is_equal_condition(full_value, json_dependency.condition)) {
                            for (const parameter in json_dependency.parameters) {
                                let value = json_dependency.parameters[parameter].value.value;
                                // if (ftd_utils.isJson(value)) {
                                //     value = JSON.parse(value);
                                // }
                                if (!!value && !!value["$kind$"]) {
                                    value = value[value["$kind$"]];
                                }
                                let important = json_dependency.parameters[parameter].value.important;
                                ftd_utils.set_style(parameter, `${dependency}:${id}`, value, important);
                                if (!styles_edited.includes(`${parameter}::${dependency}`)) {
                                    styles_edited.push(`${parameter}::${dependency}`);
                                }
                            }
                        } else {
                            for (const parameter in json_dependency.parameters) {
                                let default_value = json_dependency.parameters[parameter].default;
                                // if (ftd_utils.isJson(default_value)) {
                                //     default_value = JSON.parse(default_value);
                                // }
                                if (!!default_value && !!default_value["$kind$"]) {
                                    default_value = default_value[default_value["$kind$"]];
                                }
                                if (!styles_edited.includes(`${parameter}::${dependency}`)) {
                                    if (default_value === null) {
                                        if (["border-left-width", "border-right-width", "border-top-width", "border-bottom-width"].includes(parameter)) {
                                            default_value = "0px";
                                            document.querySelector(`[data-id="${dependency}:${id}"]`).style[`${parameter}`] = default_value;
                                        } else {
                                            ftd_utils.set_style(parameter, `${dependency}:${id}`, default_value, false);
                                        }
                                    } else {
                                        let value = default_value.value;
                                        // if (ftd_utils.isJson(value)) {
                                        //     value = JSON.parse(value);
                                        // }
                                        if (!!value && !!value["$kind$"]) {
                                            value = value[value["$kind$"]];
                                        }
                                        let important = default_value.important;
                                        ftd_utils.set_style(parameter, `${dependency}:${id}`, value, important);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if (visibility_change) {
                ftd_utils.external_children_replace(id, ftd_external_children);
            }
        }
    },

    external_children_replace: function (id, ftd_external_children) {
        if (ftd_external_children[id] === undefined) {
            return;
        }
        let external_children = ftd_external_children[id];
        let external_children_placed = [];
        for (const object in external_children) {
            if (!external_children.hasOwnProperty(object)) {
                continue;
            }

            let conditions = external_children[object];
            for (const idx in conditions) {
                if (!conditions.hasOwnProperty(idx)) {
                    continue;
                }

                let condition = conditions[idx].condition;
                let set_at = conditions[idx].set_at;
                let display = true;
                for (const i in condition) {
                    if (!condition.hasOwnProperty(i)) {
                        continue;
                    }

                    display &= ftd_utils.is_visible(id, condition[i]);
                    if (!display) {
                        break;
                    }
                }
                if (display && !external_children_placed.includes(object)) {
                    console_log(`${object}:${id}::: ${set_at}:${id}`);
                    let get_element_set_at = document.querySelector(`[data-id="${set_at}:${id}"]`);
                    let objects_to_set = document.querySelectorAll(`[data-ext-id="${object}:${id}"]`);
                    for (let i = 0; i < objects_to_set.length; i++) {
                        let object_to_set = objects_to_set[i];
                        let parent = object_to_set.parentElement;
                        if (parent !== get_element_set_at) {
                            get_element_set_at.appendChild(object_to_set);
                        }
                    }
                    external_children_placed.push(object);
                }
            }

        }
    }
};

window.ftd = (function () {
    let ftd_data = {};
    let ftd_external_children = {};

    // Setting up default value on <input>
    const inputElements = document.querySelectorAll('input[data-dv]');
    for (let input_ele of inputElements) {
        input_ele.defaultValue = input_ele.dataset.dv;
    }

    function handle_event(evt, id, action, obj) {
        let act = action["action"];
        let data = ftd_data[id];
        if (act === "stop-propagation") {
            evt.stopPropagation();
        } else if (act === "prevent-default") {
            evt.preventDefault();
        } else if (act === "toggle") {
            let target = action["target"];
            let var_name = ftd_utils.get_name_and_remaining(target)[0];
            let value = data[var_name].value;
            if (typeof value === "string" || value instanceof String) {
                value = value === 'true';
            }
            exports.set_bool(id, target, !value);
        } else if (act === "message-host") {
            if (action["parameters"].data !== undefined) {
                let value = action["parameters"].data[0].value;
                let reference = JSON.parse(action["parameters"].data[0].reference);
                let filtered_data = filter_keys(reference, (key) => !key.startsWith("$"));
                let resolved_data = ftd_utils.resolve_reference(value, filtered_data, data, obj);
                let func = resolved_data.function? resolved_data.function.trim().replaceAll("-", "_").toLowerCase(): "http";
                window[func](id, resolved_data, reference);
            } else {
                let target = action["target"].trim().replaceAll("-", "_");
                window[target](id);
            }
        } else if (act === "increment") {
            let target = action["target"];
            let increment = 1;
            if (action["parameters"].by !== undefined) {
                let by_value = action["parameters"].by[0].value;
                let by_reference = action["parameters"].by[0].reference;
                increment = parseInt(ftd_utils.resolve_reference(by_value, by_reference, data, obj));
            }
            let clamp_max = undefined;
            let clamp_min = undefined;
            if (action["parameters"]["clamp"] !== undefined) {
                let clamp_value = action["parameters"]["clamp"];
                if (clamp_value.length === 1) {
                    clamp_max = parseInt(ftd_utils.resolve_reference(clamp_value[0].value, clamp_value[0].reference, data, obj));
                }
                if (clamp_value.length === 2) {
                    clamp_min = parseInt(ftd_utils.resolve_reference(clamp_value[0].value, clamp_value[0].reference, data, obj));
                    clamp_max = parseInt(ftd_utils.resolve_reference(clamp_value[1].value, clamp_value[1].reference, data, obj));
                }
            }
            exports.increment_decrement_value(id, target, increment, clamp_min, clamp_max);

        } else if (act === "decrement") {
            let target = action["target"];
            let decrement = -1;
            if (action["parameters"].by !== undefined) {
                let by_value = action["parameters"].by[0].value;
                let by_reference = action["parameters"].by[0].reference;
                decrement = -parseInt(ftd_utils.resolve_reference(by_value, by_reference, data, obj));
            }

            let clamp_max = undefined;
            let clamp_min = undefined;
            if (action["parameters"]["clamp"] !== undefined) {
                let clamp_value = action["parameters"]["clamp"];
                if (clamp_value.length === 1) {
                    clamp_max = parseInt(ftd_utils.resolve_reference(clamp_value[0].value, clamp_value[0].reference, data, obj));
                }
                if (clamp_value.length === 2) {
                    clamp_min = parseInt(ftd_utils.resolve_reference(clamp_value[0].value, clamp_value[0].reference, data, obj));
                    clamp_max = parseInt(ftd_utils.resolve_reference(clamp_value[1].value, clamp_value[1].reference, data, obj));
                }
            }

            exports.increment_decrement_value(id, target, decrement, clamp_min, clamp_max);
        } else if (act === "set-value") {
            let target = action["target"];
            let value_data = action["parameters"].value[0];
            let value = ftd_utils.resolve_reference(value_data.value, value_data.reference, data, obj)
            if (action["parameters"].value[1].value === "integer") {
                value = parseInt(value.toString());
            } else if (action["parameters"].value[1].value === "decimal") {
                value = parseFloat(value.toString());
            } else if (action["parameters"].value[1].value === "boolean") {
                value = (value === "true") || value;
            } else if (ftd_utils.isJson(value)) {
                value = JSON.parse(value)
            }

            let var_name = ftd_utils.get_name_and_remaining(target)[0];
            if (!data[var_name]) {
                console_log(target, " is not in data, ignoring");
                return;
            }
            let old_value = ftd_utils.get_data_value(data, target);
            if (ftd_utils.deepEqual(old_value, value) && !target.includes("MOUSE-IN")) {
                console_log(target, " value is same as current, ignoring");
                return;
            }
            ftd_utils.handle_action(id, target, value, data, ftd_external_children);

        } else if (act === "insert") {
            let target = action["target"];
            let value = undefined;
            if (action["parameters"].value !== undefined) {
                let insert_value = action["parameters"].value[0].value;
                let insert_reference = action["parameters"].value[0].reference;
                value = ftd_utils.resolve_reference(insert_value, insert_reference, data, obj);
            }
            let at = undefined;
            if (action["parameters"].at !== undefined) {
                let at_value = action["parameters"].at[0].value;
                let at_reference = action["parameters"].at[0].reference;
                at = ftd_utils.resolve_reference(at_value, at_reference, data, obj);
            }

            exports.insert_value(id, target, value, at);

        } else if (act === "clear") {
            let target = action["target"];
            let value = "";
            let var_name = ftd_utils.get_name_and_remaining(target)[0];
            if (data[var_name].value instanceof Object) {
                let list = [];
                value = list;
            }
            ftd_utils.handle_action(id, target, value, data, ftd_external_children);
        } else {
            console_log("unknown action:", act);
            return;
        }

    }

    let exports = {};

    exports.handle_event = function (evt, id, event, obj) {
        console_log(id, event);
        let actions = JSON.parse(event);
        for (const action in actions) {
            handle_event(evt, id, actions[action], obj)
        }
    }

    exports.increment_decrement_value = function (id, variable, increment_by, clamp_min, clamp_max) {
        let data = ftd_data[id];

        if (!data[variable]) {
            console_log(variable, "is not in data, ignoring");
            return;
        }

        let value = parseInt(ftd_utils.get_data_value(data, variable));
        value += increment_by;

        if (clamp_max !== undefined) {
            let min = (clamp_min === undefined) ? 0: clamp_min
            if (clamp_max < value) {
                value = min;
            }
            if (clamp_min > value) {
                value = clamp_max;
            }
        }

        ftd_utils.handle_action(id, variable, value, data, ftd_external_children);
    }

    exports.insert_value = function (id, target, value, at) {
        let data = ftd_data[id];


        if (!data[ftd_utils.get_name_and_remaining(target)[0]]) {
            console_log(target, "is not in data, ignoring");
            return;
        }

        let list = ftd_utils.get_data_value(data, target);

        if (!(list instanceof Object)) {
            console_log(list, "is not list, ignoring");
            return;
        }

        if (value === undefined || value.trim() === "") {
            console_log("Nothing to insert in ", list);
            return;
        }

        if (at !== undefined && at === "end") {
            list.push(value);
        } else if (at !== undefined && at === "start") {
            list.unshift(value);
        } else {
            list.push(value);
        }

        ftd_utils.handle_action(id, target, list, data, ftd_external_children);
    }

    exports.set_bool = function (id, variable, value) {
        let data = ftd_data[id];

        if (!data[variable]) {
            console_log(variable, "is not in data, ignoring");
            return;
        }

        ftd_utils.handle_action(id, variable, value, data, ftd_external_children);
    }

    exports.set_string = function (id, variable, value) {
        let data = ftd_data[id];

        if (!data[variable]) {
            console_log(variable, "is not in data, ignoring");
            return;
        }

        ftd_utils.handle_action(id, variable, value, data, ftd_external_children);
    }

    exports.get_value = function (id, variable) {
        let data = ftd_data[id];

        let var_name = ftd_utils.get_name_and_remaining(variable)[0];

        if (!data[var_name]) {
            console_log(variable, "is not in data, ignoring");
            return;
        }
        return ftd_utils.get_data_value(data, variable);
    }

    exports.set_multi_value = function (id, list) {
        for (const idx in list) {
            if (!list.hasOwnProperty(idx)) {
                continue;
            }

            let item = list[idx];
            let [variable, value] = item;
            this.set_bool(id, variable, value);
        }
    }

    exports.init = function (id, data, external_children) {
        ftd_data[id] = JSON.parse(document.getElementById(data).innerText);
        ftd_external_children[id] = JSON.parse(document.getElementById(external_children).innerText);
        window.ftd.post_init();
    }

    exports.set_bool_for_all = function (variable, value) {
        for (let id in ftd_data) {
            if (!ftd_data.hasOwnProperty(id)) {
                continue;
            }

            exports.set_bool(id, variable, value)
        }
    }

    exports.set_string_for_all = function (variable, value) {
        for (let id in ftd_data) {
            if (!ftd_data.hasOwnProperty(id)) {
                continue;
            }

            exports.set_string(id, variable, value)
        }
    }

    exports.set_value = function (id, variable, value) {
        let data = ftd_data[id];
        if (!data[variable]) {
            console_log(variable, "is not in data, ignoring");
            return;
        }
        ftd_utils.handle_action(id, variable, value, data, ftd_external_children);
    }

    return exports;
})();

function filter_keys(data, filter_fn) {
    if (!(data instanceof Object)) {
        throw "data is not an object";
    }

    let o = {};

    for(const key of Object.keys(data)) {
        if (filter_fn(key)) {
            o[key] = data[key];
        }
    }

    return o;
}
function http(id, request_data, referenced_data) {
    let method = request_data.method? request_data.method.trim().toUpperCase(): "GET";

    if (method == "GET") {
        let query_parameters = new URLSearchParams();
        for (let [header, value] of Object.entries(request_data)) {
            if (header != "url" && header != "function" && header != "method")
            {
                query_parameters.set(header, value);
            }
       }
       let query_string = query_parameters.toString();
       if (query_string) {
           let get_url = request_data.url + "?" + query_parameters.toString();
           window.location.href = get_url;
       }
       else{
           window.location.href = request_data.url;
       }
       return;
    }

    let xhr = new XMLHttpRequest();
    xhr.open(method, request_data.url);
    xhr.setRequestHeader("Accept", "application/json");
    xhr.setRequestHeader("Content-Type", "application/json");

    xhr.onreadystatechange = function () {
        if (xhr.readyState !== 4) {
            // this means request is still underway
            // https://developer.mozilla.org/en-US/docs/Web/API/XMLHttpRequest/readyState
            return;
        }

        if (xhr.status > 500) {
            console.log("Error in calling url: ", request_data.url, xhr.responseText);
            return;
        }

        let response = JSON.parse(xhr.response);
        if (!!response && !!response.redirect) {
            // Warning: we don't handle header location redirect
            window.location.href = response.redirect;
        } else if (!!response && !!response.reload) {
            window.location.reload();
        } else {
            let data = {};

            if (!!response.errors) {
                for (key of Object.keys(response.errors)) {
                    let value = response.errors[key];
                    if (Array.isArray(value)) {
                        // django returns a list of strings
                        value = value.join(" ");
                        // also django does not append `-error`
                        key = key + "-error";
                    }
                    data[key] = value;
                }
            }

            if (!!response.data) {
                if (!!data) {
                    console_log("both .errrors and .data are present in response, ignoring .data");
                } else {
                    data = response.data;
                }
            }

            for (ftd_variable of Object.keys(data)) {
                if (referenced_data["$" + ftd_variable]) {
                    ftd_variable = referenced_data["$" + ftd_variable];
                }
                ftd.set_value(id, ftd_variable, data[key])
            }
        }
    };
    xhr.send(JSON.stringify(request_data));
}

function redirect(id, data) {
    window.location.href = data.url;
}

window.ftd.post_init = function () {
    const DARK_MODE = "ftd#dark-mode";
    const SYSTEM_DARK_MODE = "ftd#system-dark-mode";
    const FOLLOW_SYSTEM_DARK_MODE = "ftd#follow-system-dark-mode";
    const DARK_MODE_COOKIE = "ftd-dark-mode";
    const COOKIE_SYSTEM_LIGHT = "system-light";
    const COOKIE_SYSTEM_DARK = "system-dark";
    const COOKIE_DARK_MODE = "dark";
    const COOKIE_LIGHT_MODE = "light";
    const DARK_MODE_CLASS = "fpm-dark";
    const MOBILE_CLASS = "ftd-mobile";
    const XL_CLASS = "ftd-xl";
    const FTD_DEVICE = "ftd#device";
    const FTD_MOBILE_BREAKPOINT = "ftd#mobile-breakpoint";
    const FTD_DESKTOP_BREAKPOINT = "ftd#desktop-breakpoint";
    const FTD_THEME_COLOR = "ftd#theme-color";
    const THEME_COLOR_META = "theme-color";
    const MARKDOWN_COLOR = "ftd#markdown-color";
    const MARKDOWN_BACKGROUND_COLOR = "ftd#markdown-background-color";
    let last_device;

    function initialise_device() {
        last_device = get_device();
        console_log("last_device", last_device);
        window.ftd.set_bool_for_all(FTD_DEVICE, last_device);
    }

    window.onresize = function () {
        let current = get_device();
        if (current === last_device) {
            return;
        }

        window.ftd.set_string_for_all(FTD_DEVICE, current);
        last_device = current;
        console_log("last_device", last_device);
    }

    function update_markdown_colors() {
        // remove all colors from ftd.css: copy every deleted stuff in this function
        let markdown_style_sheet = document.createElement('style');


        markdown_style_sheet.innerHTML = `
        .ft_md a {
            color: ${window.ftd.get_value("main", MARKDOWN_COLOR + ".link.light")};
            background-color: ${window.ftd.get_value("main", MARKDOWN_BACKGROUND_COLOR + ".link.light")};
        }
        body.fpm-dark .ft_md a {
            color: ${window.ftd.get_value("main", MARKDOWN_COLOR + ".link.dark")};
            background-color: ${window.ftd.get_value("main", MARKDOWN_BACKGROUND_COLOR + ".link.dark")};
        }
        
        .ft_md code {
            color: ${window.ftd.get_value("main", MARKDOWN_COLOR + ".code.light")};
            background-color: ${window.ftd.get_value("main", MARKDOWN_BACKGROUND_COLOR + ".code.light")};
        }
        body.fpm-dark .ft_md code {
            color: ${window.ftd.get_value("main", MARKDOWN_COLOR + ".code.dark")};
            background-color: ${window.ftd.get_value("main", MARKDOWN_BACKGROUND_COLOR + ".code.dark")};
        }        
                
        .ft_md a:visited {
            color: ${window.ftd.get_value("main", MARKDOWN_COLOR + ".link-visited.light")};
            background-color: ${window.ftd.get_value("main", MARKDOWN_BACKGROUND_COLOR + ".link-visited.light")};
        }      
        body.fpm-dark .ft_md a:visited {
            color: ${window.ftd.get_value("main", MARKDOWN_COLOR + ".link-visited.dark")};
            background-color: ${window.ftd.get_value("main", MARKDOWN_BACKGROUND_COLOR + ".link-visited.dark")};
        }
            
        .ft_md a code {
            color: ${window.ftd.get_value("main", MARKDOWN_COLOR + ".link-code.light")};
            background-color: ${window.ftd.get_value("main", MARKDOWN_BACKGROUND_COLOR + ".link-code.light")};
        }
        body.fpm-dark .ft_md a code {
            color: ${window.ftd.get_value("main", MARKDOWN_COLOR + ".link-code.dark")};
            background-color: ${window.ftd.get_value("main", MARKDOWN_BACKGROUND_COLOR + ".link-code.dark")};
        }
                
        .ft_md a:visited code {
            color: ${window.ftd.get_value("main", MARKDOWN_COLOR + ".link-visited-code.light")};
            background-color: ${window.ftd.get_value("main", MARKDOWN_BACKGROUND_COLOR + ".link-visited-code.light")};
        }
        body.fpm-dark .ft_md a:visited code {
            color: ${window.ftd.get_value("main", MARKDOWN_COLOR + ".link-visited-code.dark")};
            background-color: ${window.ftd.get_value("main", MARKDOWN_BACKGROUND_COLOR + ".link-visited-code.dark")};            
        }
        
        .ft_md ul ol li:before {
            color: ${window.ftd.get_value("main", MARKDOWN_COLOR + ".ul-ol-li-before.light")};
            background-color: ${window.ftd.get_value("main", MARKDOWN_BACKGROUND_COLOR + ".ul-ol-li-before.light")};
        }     
        body.fpm-dark .ft_md ul ol li:before {
            color: ${window.ftd.get_value("main", MARKDOWN_COLOR + ".ul-ol-li-before.dark")};
            background-color: ${window.ftd.get_value("main", MARKDOWN_BACKGROUND_COLOR + ".ul-ol-li-before.dark")};
        }     
        `;

        document.getElementsByTagName('head')[0].appendChild(markdown_style_sheet);
    }

    function get_device() {
        // not at all sure about this functions logic.
        let width = window.innerWidth;

        // in future we may want to have more than one break points, and then
        // we may also want the theme builders to decide where the breakpoints
        // should go. we should be able to fetch fpm variables here, or maybe
        // simply pass the width, user agent etc to fpm and let people put the
        // checks on width user agent etc, but it would be good if we can
        // standardize few breakpoints. or maybe we should do both, some
        // standard breakpoints and pass the raw data.

        // we would then rename this function to detect_device() which will
        // return one of "desktop", "tablet", "mobile". and also maybe have
        // another function detect_orientation(), "landscape" and "portrait" etc,
        // and instead of setting `fpm#mobile: boolean` we set `fpm-ui#device`
        // and `fpm#view-port-orientation` etc.
        let mobile_breakpoint = window.ftd.get_value("main", FTD_MOBILE_BREAKPOINT);
        let desktop_breakpoint = window.ftd.get_value("main", FTD_DESKTOP_BREAKPOINT);
        if (width <= mobile_breakpoint) {
            document.body.classList.add(MOBILE_CLASS);
            if (document.body.classList.contains(XL_CLASS)) {
                document.body.classList.remove(XL_CLASS);
            }
            return "mobile";
        }
        if (width > desktop_breakpoint) {
            document.body.classList.add(XL_CLASS);
            if (document.body.classList.contains(MOBILE_CLASS)) {
                document.body.classList.remove(MOBILE_CLASS);
            }
            return "xl";
        }
        if (document.body.classList.contains(MOBILE_CLASS)) {
            document.body.classList.remove(MOBILE_CLASS);
        }
        if (document.body.classList.contains(XL_CLASS)) {
            document.body.classList.remove(XL_CLASS);
        }
        return "desktop";
    }

    /*
        ftd.dark-mode behaviour:

        ftd.dark-mode is a boolean, default false, it tells the UI to show
        the UI in dark or light mode. Themes should use this variable to decide
        which mode to show in UI.

        ftd.follow-system-dark-mode, boolean, default true, keeps track if
        we are reading the value of `dark-mode` from system preference, or user
        has overridden the system preference.

        These two variables must not be set by ftd code directly, but they must
        use `$on-click$: message-host enable-dark-mode`, to ignore system
        preference and use dark mode. `$on-click$: message-host
        disable-dark-mode` to ignore system preference and use light mode and
        `$on-click$: message-host follow-system-dark-mode` to ignore user
        preference and start following system preference.

        we use a cookie: `ftd-dark-mode` to store the preference. The cookie can
        have three values:

           cookie missing /          user wants us to honour system preference
               system-light          and currently its light.

           system-dark               follow system and currently its dark.

           light:                    user prefers light

           dark:                     user prefers light

        We use cookie instead of localstorage so in future `fpm-repo` can see
        users preferences up front and renders the HTML on service wide
        following user's preference.

     */

    window.enable_dark_mode = function () {
        // TODO: coalesce the two set_bool-s into one so there is only one DOM
        //       update
        window.ftd.set_bool_for_all(DARK_MODE, true);
        window.ftd.set_bool_for_all(FOLLOW_SYSTEM_DARK_MODE, false);
        window.ftd.set_bool_for_all(SYSTEM_DARK_MODE, system_dark_mode());
        document.body.classList.add(DARK_MODE_CLASS);
        set_cookie(DARK_MODE_COOKIE, COOKIE_DARK_MODE);
        update_theme_color();
    }

    window.enable_light_mode = function () {
        // TODO: coalesce the two set_bool-s into one so there is only one DOM
        //       update
        window.ftd.set_bool_for_all(DARK_MODE, false);
        window.ftd.set_bool_for_all(FOLLOW_SYSTEM_DARK_MODE, false);
        window.ftd.set_bool_for_all(SYSTEM_DARK_MODE, system_dark_mode());
        if (document.body.classList.contains(DARK_MODE_CLASS)) {
            document.body.classList.remove(DARK_MODE_CLASS);
        }
        set_cookie(DARK_MODE_COOKIE, COOKIE_LIGHT_MODE);
        update_theme_color();
    }

    window.enable_system_mode = function () {
        // TODO: coalesce the two set_bool-s into one so there is only one DOM
        //       update
        window.ftd.set_bool_for_all(FOLLOW_SYSTEM_DARK_MODE, true);
        window.ftd.set_bool_for_all(SYSTEM_DARK_MODE, system_dark_mode());
        if (system_dark_mode()) {
            window.ftd.set_bool_for_all(DARK_MODE, true);
            document.body.classList.add(DARK_MODE_CLASS);
            set_cookie(DARK_MODE_COOKIE, COOKIE_SYSTEM_DARK)
        } else {
            window.ftd.set_bool_for_all(DARK_MODE, false);
            if (document.body.classList.contains(DARK_MODE_CLASS)) {
                document.body.classList.remove(DARK_MODE_CLASS);
            }
            set_cookie(DARK_MODE_COOKIE, COOKIE_SYSTEM_LIGHT)
        }
        update_theme_color();
    }


    window.ftd_warnings_delivered = {};

    window["ls.set_boolean"] = function(_id, resolved_data, reference) {
        // TODO: we need a better way to detect absence of localStorage
        if (!isLocalStorageAvailable()) {
            // we are doing this business so we only show the warning once in console
            if (window.ftd_warnings_delivered["ls.set_boolean"]) {
                return;
            }
            window.ftd_warnings_delivered["ls.set_boolean"] = true;

            // TODO: we should have `ftd.localStorage-present` or some such, so we can
            //       show a warning in UI as well, instead of just in console.

            console.warn("localStorage not available, but ls.set-boolean was used");
            return;
        }

        // TODO: we are not using id yet.
        console.log("ls.set-boolean", reference["variable"], resolved_data["variable"]);
        window.localStorage.setItem(reference["variable"], resolved_data["variable"]);
    }

    // source: https://stackoverflow.com/questions/16427636/check-if-localstorage-is-available
    function isLocalStorageAvailable(){
        if (!window.localStorage) {
            return false;
        }

        var test = 'test';
        try {
            localStorage.setItem(test, test);
            localStorage.removeItem(test);
            return true;
        } catch(e) {
            return false;
        }
    }

    function scan_local_store() {
        if (!isLocalStorageAvailable()) {
            return;
        }

        Object.keys(window.localStorage).forEach(function(key){
            let value = window.localStorage.getItem(key);
            // value would be def string or null
            // we want to set a boolean variable using `value`
            // we want to only do if everything is alright
            if (value === "true" || value === "false") {
                // TODO: how do we handle id? we are hard-coding main here, which is wrong
                window.ftd.set_value("main", key,  value === "true");
            } else {
                console.log("contract violation, expected boolean", key, "found", value);
                // TODO: use key prefixes so we can delete the keys safely
            }
        });
    }

    function update_theme_color() {
        let theme_color = window.ftd.get_value("main", FTD_THEME_COLOR);
        if (!!theme_color) {
            document.body.style.backgroundColor = FTD_THEME_COLOR;
            set_meta(THEME_COLOR_META, theme_color);
        } else {
            document.body.style.backgroundColor = FTD_THEME_COLOR;
            delete_meta(THEME_COLOR_META);
        }
    }

    function set_meta(name, value) {
        let meta = document.querySelector("meta[name=" + name + "]");
        if (!!meta) {
            meta.content = value;
        } else {
            meta = document.createElement('meta');
            meta.name = name;
            meta.content = value;
            document.getElementsByTagName('head')[0].appendChild(meta);
        }
    }

    function delete_meta(name) {
        let meta = document.querySelector("meta[name=" + name + "]")
        if (!!meta) {
            meta.remove();
        }
    }

    function set_cookie(name, value) {
        document.cookie = name + "=" + value + "; path=/";
    }

    function system_dark_mode() {
        return !!(window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches)
    }

    function initialise_dark_mode() {
        update_dark_mode();
        start_watching_dark_mode_system_preference();
    }

    function get_cookie(name, def) {
        // source: https://stackoverflow.com/questions/5639346/
        let regex = document.cookie.match('(^|;)\\s*' + name + '\\s*=\\s*([^;]+)');
        return regex !== null ? regex.pop() : def;
    }

    function update_dark_mode() {
        let current_dark_mode_cookie = get_cookie(DARK_MODE_COOKIE, COOKIE_SYSTEM_LIGHT);

        switch (current_dark_mode_cookie) {
            case COOKIE_SYSTEM_LIGHT:
            case COOKIE_SYSTEM_DARK:
                window.enable_system_mode();
                break;
            case COOKIE_LIGHT_MODE:
                window.enable_light_mode();
                break;
            case COOKIE_DARK_MODE:
                window.enable_dark_mode();
                break;
            default:
                console_log("cookie value is wrong", current_dark_mode_cookie);
                window.enable_system_mode();
        }
    }

    function start_watching_dark_mode_system_preference() {
        window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').addEventListener(
            "change", update_dark_mode
        );
    }
    initialise_dark_mode();
    initialise_device();
    update_markdown_colors();
    scan_local_store();
};
