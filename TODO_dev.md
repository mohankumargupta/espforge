1. need to force release-plz to publish in this order:
     
espforge_common 
espforge_esp32metadata 
espforge_configuration
espforge_codegen 
espforge_platform
espforge_components
espforge_macros 
espforge_components_builder
espforge_devices
espforge_devices_builder
espforge_dialogue
espforge_examples
espforge

it currently tries to publish espforge_examples too early.

