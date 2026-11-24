<!DOCTYPE html>
<html>
<body>
<?php
// Greets the visitor.
function greet(string $name): string
{
    if ($name === '') {
        return "Hello, world";
    }
    return "Hello, " . $name;
}
echo greet('vr');
?>
</body>
</html>
