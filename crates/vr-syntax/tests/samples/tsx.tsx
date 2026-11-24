// A greeting component.
interface Props {
  name: string;
}

export function Hello({ name }: Props) {
  const title = "greeting";
  return <div className="hello" title={title}>Hello, {name}</div>;
}
