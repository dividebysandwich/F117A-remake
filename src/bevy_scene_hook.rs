use bevy::{
    prelude::*,
    scene::SceneInstance,
    ecs::{world::EntityRef, system::EntityCommands},
};
#[derive(Component, Debug)]
pub struct SceneHooked;

#[derive(Component)]
pub struct SceneHook {
    hook: Box<dyn Fn(&EntityRef, &mut EntityCommands) + Send + Sync + 'static>,
}
impl SceneHook {
    pub fn new<F: Fn(&EntityRef, &mut EntityCommands) + Send + Sync + 'static>(hook: F) -> Self {
        Self { hook: Box::new(hook) }
    }
}

/// Bundle a [`SceneHook`] with the standard [`SceneBundle`] components.
///
/// See [`HookedDynamicSceneBundle`] for dynamic scene support.
#[derive(Bundle)]
#[allow(missing_docs /* field description is trivial */)]
pub struct HookedSceneBundle {
    pub hook: SceneHook,
    pub scene: SceneBundle,
}

/// Bundle a [`SceneHook`] with dynamic scenes [`DynamicSceneBundle`] components.
///
/// Similar to [`HookedSceneBundle`], but for dynamic scenes.
#[derive(Bundle)]
#[allow(missing_docs /* field description is trivial */)]
pub struct HookedDynamicSceneBundle {
    pub hook: SceneHook,
    pub scene: DynamicSceneBundle,
}

pub fn run_hooks(
    unloaded_instances: Query<(Entity, &SceneInstance, &SceneHook), Without<SceneHooked>>,
    scene_manager: Res<SceneSpawner>,
    world: &World,
    mut cmds: Commands,
) {
    for (entity, instance, hooked) in unloaded_instances.iter() {
        if scene_manager.instance_is_ready(**instance) {
            cmds.entity(entity).insert(SceneHooked);
        }
        let entities = scene_manager
            .iter_instance_entities(**instance)
            .chain(std::iter::once(entity));
        for entity_ref in entities.filter_map(|e| world.get_entity(e)) {
            let mut cmd = cmds.entity(entity_ref.id());
            (hooked.hook)(&entity_ref, &mut cmd);
        }
    }
}

pub struct HookPlugin;
impl Plugin for HookPlugin {
    fn build(&self, app: &mut App) { app.add_systems(Update, run_hooks); }
}
